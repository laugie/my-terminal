use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use super::binary::SSH_NOT_FOUND;
use super::io::read_chunk;
use super::SessionEvent;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Shutdown waits this long for the reaper after the PTY ends are released.
/// On timeout the join is detached so close cannot block forever.
const REAPER_JOIN_TIMEOUT: Duration = Duration::from_secs(3);

/// How long release waits if a write or resize still holds the end.
const END_LOCK_WAIT: Duration = Duration::from_millis(250);

type PtyWriter = Box<dyn std::io::Write + Send>;
type PtyMaster = Box<dyn MasterPty + Send>;

/// One running ssh process and the threads that move its output.
pub struct LiveSession {
    pub id: String,
    /// `None` once shutdown has released the write end.
    pub writer: Arc<Mutex<Option<PtyWriter>>>,
    /// `None` once shutdown has released the master.
    ///
    /// ConPTY does not EOF a blocked read until both ends are dropped.
    pub master: Arc<Mutex<Option<PtyMaster>>>,
    kill_tx: Option<mpsc::Sender<()>>,
    reaper: Option<JoinHandle<()>>,
}

impl LiveSession {
    /// Stop the child, release the PTY ends, and wait until it is reaped.
    pub fn shutdown(mut self) -> Result<(), String> {
        self.finish()
    }

    fn finish(&mut self) -> Result<(), String> {
        self.signal_stop();
        // Drop the ends before joining. On Windows the reader never sees EOF
        // while the master or writer is still open, so joining first deadlocks.
        self.release_ends();
        self.join_reaper()
    }

    fn signal_stop(&mut self) {
        if let Some(tx) = self.kill_tx.take() {
            let _ = tx.send(());
        }
    }

    fn release_ends(&self) {
        take_end(&self.writer);
        take_end(&self.master);
    }

    fn join_reaper(&mut self) -> Result<(), String> {
        if let Some(handle) = self.reaper.take() {
            return join_with_timeout(handle);
        }
        Ok(())
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// Open a PTY and spawn `program` with `args` directly.
///
/// `program` is the absolute path of the system ssh binary. Arguments are
/// appended one by one; the shell is not involved. Callers outside this crate
/// cannot pass a program: the only command that starts a session resolves ssh
/// itself.
pub(crate) fn start_pty(
    program: &Path,
    args: &[String],
    on_event: impl Fn(SessionEvent) -> Result<(), ()> + Send + 'static,
) -> Result<LiveSession, String> {
    if !program.is_absolute() {
        return Err(SSH_NOT_FOUND.to_string());
    }

    let mut command = CommandBuilder::new(program);
    command.args(args);

    let system = native_pty_system();
    let pair = system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|_| "无法创建终端".to_string())?;
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|_| "启动 ssh 失败".to_string())?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|_| "无法读取终端输出".to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|_| "无法写入终端".to_string())?;
    let master = pair.master;
    drop(pair.slave);

    let (event_tx, event_rx) = mpsc::channel();
    let reader_tx = event_tx.clone();
    let reader_handle =
        match thread::Builder::new()
            .name("pty-reader".to_string())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match read_chunk(&mut reader, &mut buf) {
                        Ok(Some(bytes)) => {
                            if reader_tx.send(SessionEvent::Data { bytes }).is_err() {
                                break;
                            }
                        }
                        Ok(None) | Err(_) => break,
                    }
                }
            }) {
            Ok(handle) => handle,
            Err(_) => {
                // `reader` moved into the closure, which is dropped on spawn failure.
                drop(writer);
                drop(master);
                let _ = terminate(child);
                return Err("无法启动会话线程".to_string());
            }
        };

    let (kill_tx, kill_rx) = mpsc::channel::<()>();
    type Boot = (
        Box<dyn Child + Send + Sync>,
        mpsc::Receiver<()>,
        JoinHandle<()>,
        mpsc::Sender<SessionEvent>,
    );
    let (boot_tx, boot_rx) = mpsc::channel::<Boot>();
    let reaper = match thread::Builder::new()
        .name("pty-reaper".to_string())
        .spawn(move || {
            let Ok((child, kill_rx, reader_handle, event_tx)) = boot_rx.recv() else {
                return;
            };
            let code = wait_or_kill(child, kill_rx);
            // Publish the exit before joining the reader. The reader blocks
            // until the master and writer are dropped, which shutdown does
            // only after this event has been queued.
            let _ = event_tx.send(SessionEvent::Exit { code });
            let _ = reader_handle.join();
        }) {
        Ok(handle) => handle,
        Err(_) => {
            drop(writer);
            drop(master);
            let _ = join_with_timeout(reader_handle);
            let _ = terminate(child);
            return Err("无法启动会话线程".to_string());
        }
    };

    if let Err(err) = boot_tx.send((child, kill_rx, reader_handle, event_tx)) {
        let (child, _, reader_handle, _) = err.0;
        drop(writer);
        drop(master);
        let _ = join_with_timeout(reader_handle);
        let _ = join_with_timeout(reaper);
        let _ = terminate(child);
        return Err("无法启动会话线程".to_string());
    }

    // The forwarder only crosses into the UI. It is detached so shutting the
    // session down never waits on the webview.
    match thread::Builder::new()
        .name("pty-forward".to_string())
        .spawn(move || {
            while let Ok(event) = event_rx.recv() {
                if on_event(event).is_err() {
                    break;
                }
            }
        }) {
        Ok(handle) => {
            // Detach. Waiting for the UI here can deadlock window shutdown.
            drop(handle);
        }
        Err(_) => {
            drop(writer);
            drop(master);
            let _ = kill_tx.send(());
            let _ = join_with_timeout(reaper);
            return Err("无法启动会话线程".to_string());
        }
    }

    let id = format!("session-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    Ok(LiveSession {
        id,
        writer: Arc::new(Mutex::new(Some(writer))),
        master: Arc::new(Mutex::new(Some(master))),
        kill_tx: Some(kill_tx),
        reaper: Some(reaper),
    })
}

/// Take the PTY end out of the mutex and drop it.
///
/// A blocking lock here can stall behind `write_all`. Give up after a short
/// wait so the timed reaper join still runs.
fn take_end<T>(slot: &Mutex<Option<T>>) {
    let deadline = Instant::now() + END_LOCK_WAIT;
    loop {
        match slot.try_lock() {
            Ok(mut guard) => {
                drop(guard.take());
                return;
            }
            Err(TryLockError::Poisoned(poisoned)) => {
                drop(poisoned.into_inner().take());
                return;
            }
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Join `handle`, or detach it when it is still running after the timeout.
fn join_with_timeout(handle: JoinHandle<()>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let waiter = thread::Builder::new()
        .name("pty-join".to_string())
        .spawn(move || {
            let _ = tx.send(handle.join());
        });
    let Ok(waiter) = waiter else {
        // Spawn failed and dropped `handle`, which detaches that thread.
        return Ok(());
    };
    match rx.recv_timeout(REAPER_JOIN_TIMEOUT) {
        Ok(Ok(())) => {
            let _ = waiter.join();
            Ok(())
        }
        Ok(Err(_)) => {
            let _ = waiter.join();
            Err("会话线程异常退出".to_string())
        }
        Err(_) => {
            // Still running. Detach the waiter; do not block the caller.
            drop(waiter);
            Ok(())
        }
    }
}

fn wait_or_kill(
    mut child: Box<dyn Child + Send + Sync>,
    kill_rx: mpsc::Receiver<()>,
) -> Option<i32> {
    loop {
        match kill_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return terminate(child),
            Err(mpsc::RecvTimeoutError::Timeout) => match child.try_wait() {
                Ok(Some(status)) => return Some(status.exit_code() as i32),
                Ok(None) => continue,
                Err(_) => return terminate(child),
            },
        }
    }
}

/// Ask the child to exit, then wait until it has been reaped.
///
/// The first signal is the independent killer, which does not collect the
/// status. `wait` / `try_wait` on this thread owns the exit code. If the
/// process is still alive after a short grace period, the full stop is used.
fn terminate(mut child: Box<dyn Child + Send + Sync>) -> Option<i32> {
    let mut killer = child.clone_killer();
    let _ = killer.kill();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.exit_code() as i32),
            Ok(None) if started.elapsed() < Duration::from_secs(2) => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                return match child.try_wait() {
                    Ok(Some(status)) => Some(status.exit_code() as i32),
                    _ => match child.wait() {
                        Ok(status) => Some(status.exit_code() as i32),
                        Err(_) => Some(1),
                    },
                };
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Drives `start_pty` with a stand-in program. Not a frontend command.
    #[test]
    fn pty_session_emits_output_and_exit_code_then_shutdown() {
        let (program, args) = short_lived_program();
        let (tx, rx) = mpsc::channel();
        let session = start_pty(&program, &args, move |event| tx.send(event).map_err(|_| ()))
            .expect("start_pty");

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        let mut exit_code = None;
        let mut answered_cursor = false;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now());
            if remain.is_zero() {
                break;
            }
            match rx.recv_timeout(remain) {
                Ok(SessionEvent::Data { bytes }) => output.extend(bytes),
                Ok(SessionEvent::Exit { code }) => exit_code = Some(code),
                Err(_) => break,
            }
            // ConPTY asks for the cursor position and waits before the child
            // prints or exits. xterm answers this in the app; the test must
            // answer it on the PTY writer.
            answer_cursor_query(&session, &output, &mut answered_cursor);
            if bytes_contain_hi(&output) && exit_code == Some(Some(3)) {
                break;
            }
        }

        let started = Instant::now();
        let shutdown_result = session.shutdown();
        let elapsed = started.elapsed();

        let text = String::from_utf8_lossy(&output);
        assert!(
            bytes_contain_hi(&output),
            "expected output containing hi within 3s, got {text:?} exit={exit_code:?}"
        );
        assert_eq!(exit_code, Some(Some(3)), "output={text:?}");
        shutdown_result.expect("shutdown");
        // The join timeout is 3s and then detaches. A hang that only unblocks
        // by detaching must fail this tighter bound.
        assert!(
            elapsed < Duration::from_secs(2),
            "shutdown took {elapsed:?}"
        );
    }

    fn bytes_contain_hi(output: &[u8]) -> bool {
        String::from_utf8_lossy(output).contains("hi")
    }

    fn answer_cursor_query(session: &LiveSession, output: &[u8], answered: &mut bool) {
        const QUERY: &[u8] = b"\x1b[6n";
        if *answered || output.windows(QUERY.len()).all(|window| window != QUERY) {
            return;
        }
        *answered = true;
        let mut slot = session.writer.lock().unwrap_or_else(|err| err.into_inner());
        let Some(writer) = slot.as_mut() else {
            return;
        };
        use std::io::Write;
        let _ = writer.write_all(b"\x1b[1;1R");
        let _ = writer.flush();
    }

    fn short_lived_program() -> (PathBuf, Vec<String>) {
        if cfg!(windows) {
            let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
            let program = PathBuf::from(root).join("System32").join("cmd.exe");
            (
                program,
                vec!["/c".to_string(), "echo hi & exit 3".to_string()],
            )
        } else {
            (
                PathBuf::from("/bin/sh"),
                vec!["-c".to_string(), "printf hi; exit 3".to_string()],
            )
        }
    }
}

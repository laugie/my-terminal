use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use super::binary::SSH_NOT_FOUND;
use super::io::read_chunk;
use super::SessionEvent;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// One running ssh process and the threads that move its output.
pub struct LiveSession {
    pub id: String,
    pub writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    kill_tx: Option<mpsc::Sender<()>>,
    reaper: Option<JoinHandle<()>>,
}

impl LiveSession {
    /// Stop the child, wait until it is reaped, and join the reaper thread.
    pub fn shutdown(mut self) -> Result<(), String> {
        self.signal_stop();
        self.join_reaper()
    }

    fn signal_stop(&mut self) {
        if let Some(tx) = self.kill_tx.take() {
            let _ = tx.send(());
        }
    }

    fn join_reaper(&mut self) -> Result<(), String> {
        if let Some(handle) = self.reaper.take() {
            handle.join().map_err(|_| "会话线程异常退出".to_string())?;
        }
        Ok(())
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        self.signal_stop();
        let _ = self.join_reaper();
    }
}

/// Open a PTY and spawn `program` with `args` directly.
///
/// `program` is the absolute path of the system ssh binary. Arguments are
/// appended one by one; the shell is not involved.
pub fn start_pty(
    program: &Path,
    args: &[String],
    on_event: tauri::ipc::Channel<SessionEvent>,
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
            let _ = reader_handle.join();
            let _ = event_tx.send(SessionEvent::Exit { code });
        }) {
        Ok(handle) => handle,
        Err(_) => {
            let _ = terminate(child);
            let _ = reader_handle.join();
            return Err("无法启动会话线程".to_string());
        }
    };

    if let Err(err) = boot_tx.send((child, kill_rx, reader_handle, event_tx)) {
        let (child, _, reader_handle, _) = err.0;
        let _ = terminate(child);
        let _ = reader_handle.join();
        let _ = reaper.join();
        return Err("无法启动会话线程".to_string());
    }

    // The forwarder only crosses into the UI. It is detached so shutting the
    // session down never waits on the webview.
    match thread::Builder::new()
        .name("pty-forward".to_string())
        .spawn(move || {
            while let Ok(event) = event_rx.recv() {
                if on_event.send(event).is_err() {
                    break;
                }
            }
        }) {
        Ok(handle) => {
            // Detach. Waiting for the UI here can deadlock window shutdown.
            drop(handle);
        }
        Err(_) => {
            let _ = kill_tx.send(());
            let _ = reaper.join();
            return Err("无法启动会话线程".to_string());
        }
    }

    let id = format!("session-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    Ok(LiveSession {
        id,
        writer: Arc::new(Mutex::new(writer)),
        master: Arc::new(Mutex::new(master)),
        kill_tx: Some(kill_tx),
        reaper: Some(reaper),
    })
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

pub(crate) mod args;
mod binary;
mod io;
mod pty;
mod size;

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use pty::LiveSession;
use tauri::Manager;

pub use args::{build_ssh_args, SshOptions, SshTarget};
pub use binary::{resolve_ssh_binary, resolve_ssh_binary_with, OsKind, SSH_NOT_FOUND};
pub use size::{normalize_pty_size, MAX_PTY_COLS, MAX_PTY_ROWS};

/// Largest payload accepted by one `write_session` call.
const MAX_WRITE_BYTES: usize = 64 * 1024;

/// Most sessions that may be starting or running at once.
const MAX_TABS: usize = 8;

/// `shutdown_blocking` releases every child together, then waits once.
const SHUTDOWN_DEADLINE: Duration = Duration::from_millis(3500);

/// Bytes or a terminal exit, delivered on the channel passed to `open_session`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionEvent {
    Data { bytes: Vec<u8> },
    Exit { code: Option<i32> },
}

/// Identifier of the single live session.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSessionResponse {
    pub id: String,
}

struct Pending {
    id: String,
    epoch: u64,
}

struct Inner {
    /// Bumped by `shutdown_blocking` so an in-flight open cannot install.
    epoch: u64,
    pending: Vec<Pending>,
    sessions: HashMap<String, LiveSession>,
    /// ssh `-F` files owned by a live session. Removed when that session ends.
    temp_configs: HashMap<String, std::path::PathBuf>,
}

/// Process-wide session table. At most [`MAX_TABS`] sessions may be in flight.
pub struct SessionState {
    inner: Mutex<Inner>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                epoch: 0,
                pending: Vec::new(),
                sessions: HashMap::new(),
                temp_configs: HashMap::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock still has to be able to stop the child.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(&self) -> Result<String, String> {
        let mut guard = self.lock();
        if guard.pending.len() + guard.sessions.len() >= MAX_TABS {
            return Err("最多 8 个标签".to_string());
        }
        let id = next_session_id();
        let epoch = guard.epoch;
        guard.pending.push(Pending {
            id: id.clone(),
            epoch,
        });
        Ok(id)
    }

    fn abort(&self, id: &str) {
        let mut guard = self.lock();
        guard.pending.retain(|pending| pending.id != id);
    }

    fn install(&self, id: &str, session: LiveSession) -> Result<OpenSessionResponse, LiveSession> {
        self.install_prepared(id, session, None)
            .map_err(|(session, _)| session)
    }

    /// Install `session` and, in the same lock, remember a temp ssh config.
    ///
    /// On failure the path is returned so the caller can delete it. The file
    /// is not deleted here: the caller still owns the armed guard.
    fn install_prepared(
        &self,
        id: &str,
        mut session: LiveSession,
        temp_config: Option<std::path::PathBuf>,
    ) -> Result<OpenSessionResponse, (LiveSession, Option<std::path::PathBuf>)> {
        let mut guard = self.lock();
        let Some(index) = guard.pending.iter().position(|pending| pending.id == id) else {
            return Err((session, temp_config));
        };
        let pending = guard.pending.remove(index);
        if pending.epoch != guard.epoch {
            return Err((session, temp_config));
        }
        session.id = id.to_string();
        guard.sessions.insert(id.to_string(), session);
        if let Some(path) = temp_config {
            guard.temp_configs.insert(id.to_string(), path);
        }
        Ok(OpenSessionResponse { id: id.to_string() })
    }

    fn take_temp(&self, id: &str) -> Option<std::path::PathBuf> {
        self.lock().temp_configs.remove(id)
    }

    fn take(&self, id: &str) -> Result<Option<LiveSession>, String> {
        let mut guard = self.lock();
        Ok(guard.sessions.remove(id))
    }

    fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
        if data.len() > MAX_WRITE_BYTES {
            return Err("单次写入不能超过 64 KiB".to_string());
        }
        let writer = {
            let guard = self.lock();
            match guard.sessions.get(id) {
                Some(session) => Arc::clone(&session.writer),
                None => return Err("会话不存在".to_string()),
            }
        };
        let mut slot = writer.lock().unwrap_or_else(PoisonError::into_inner);
        let writer = slot.as_mut().ok_or_else(|| "会话已结束".to_string())?;
        use std::io::Write;
        writer
            .write_all(data)
            .map_err(|_| "写入会话失败".to_string())?;
        writer.flush().map_err(|_| "写入会话失败".to_string())?;
        Ok(())
    }

    fn resize(&self, id: &str, cols: u32, rows: u32) -> Result<(), String> {
        let (cols, rows) = normalize_pty_size(cols, rows)?;
        let master = {
            let guard = self.lock();
            match guard.sessions.get(id) {
                Some(session) => Arc::clone(&session.master),
                None => return Err("会话不存在".to_string()),
            }
        };
        let slot = master.lock().unwrap_or_else(PoisonError::into_inner);
        let master = slot.as_ref().ok_or_else(|| "会话已结束".to_string())?;
        master
            .resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|_| "调整终端大小失败".to_string())?;
        Ok(())
    }

    /// Kill every live child and wait until each has been reaped.
    ///
    /// Also invalidates in-flight `open_session` calls so they will not
    /// install a process after the window has closed. Children are signalled
    /// together; one deadline covers the whole set.
    pub fn shutdown_blocking(&self) {
        let (sessions, temps) = {
            let mut guard = self.lock();
            guard.epoch = guard.epoch.wrapping_add(1);
            guard.pending.clear();
            (
                std::mem::take(&mut guard.sessions),
                std::mem::take(&mut guard.temp_configs),
            )
        };
        shutdown_all(sessions.into_values().collect());
        for path in temps.into_values() {
            remove_temp_config(&path);
        }
    }

    #[cfg(test)]
    fn live_count(&self) -> usize {
        self.lock().sessions.len()
    }

    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.lock().pending.len()
    }
}

fn next_session_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "session-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// Signal every session at once, then wait until `SHUTDOWN_DEADLINE`.
fn shutdown_all(sessions: Vec<LiveSession>) {
    if sessions.is_empty() {
        return;
    }
    let mut handles = Vec::with_capacity(sessions.len());
    for session in sessions {
        match thread::Builder::new()
            .name("pty-shutdown".to_string())
            .spawn(move || session.shutdown())
        {
            Ok(handle) => handles.push(handle),
            Err(_) => {
                // The session was dropped with the failed spawn, which reaps
                // it on this thread. Keep going so the others still stop.
            }
        }
    }
    let deadline = Instant::now() + SHUTDOWN_DEADLINE;
    for handle in handles {
        let remain = deadline.saturating_duration_since(Instant::now());
        let _ = join_shutdown(handle, remain);
    }
}

fn join_shutdown(handle: thread::JoinHandle<Result<(), String>>, timeout: Duration) {
    if timeout.is_zero() {
        drop(handle);
        return;
    }
    let (tx, rx) = mpsc::channel();
    let waiter = thread::Builder::new()
        .name("pty-shutdown-wait".to_string())
        .spawn(move || {
            let _ = tx.send(handle.join());
        });
    let Ok(waiter) = waiter else {
        return;
    };
    if rx.recv_timeout(timeout).is_ok() {
        let _ = waiter.join();
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

struct Reservation {
    app: tauri::AppHandle,
    id: String,
    armed: bool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.armed {
            self.app.state::<SessionState>().abort(&self.id);
        }
    }
}

/// Start one system ssh session.
///
/// 最多 8 个标签。碰外部进程的命令一律 async。创建 PTY 与等待子进程都在
/// 阻塞线程里完成，状态锁不跨 `.await` 持有。`JoinError` 转成错误，不 panic。
///
/// `device_name` selects a row from the local topology file. Host, user, port,
/// key path, and jump are taken from that file and checked again here. A
/// request that sets `device_name` ignores `host` / `user` / `port`.
#[tauri::command]
pub async fn open_session(
    app: tauri::AppHandle,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    device_name: Option<String>,
    on_event: tauri::ipc::Channel<SessionEvent>,
) -> Result<OpenSessionResponse, String> {
    {
        let auth = app.state::<crate::auth::AuthState>();
        crate::auth::require_access(&auth, auth.lock_enabled())
    }
        .map_err(|err| err.to_string())?;
    let mut planned = if let Some(device_name) = device_name.filter(|name| !name.is_empty()) {
        let app_for_load = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let dir = crate::config::resolve_app_config_dir(&app_for_load)
                .map_err(|_| "无法定位配置目录".to_string())?;
            crate::topology::plan_device_session(&dir, &device_name)
        })
        .await
        .map_err(|_| "读取拓扑配置失败".to_string())??
    } else {
        let host = host.ok_or_else(|| "主机不能为空".to_string())?;
        crate::topology::SessionPlan {
            args: build_ssh_args(&SshTarget { host, user, port })?,
            temp_file: None,
        }
    };
    let args = std::mem::take(&mut planned.args);
    let mut temp_file = planned.temp_file.take();
    let id = app.state::<SessionState>().begin()?;
    let mut reservation = Reservation {
        app: app.clone(),
        id: id.clone(),
        armed: true,
    };
    let app_for_task = app.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let started = (|| {
            let program = resolve_ssh_binary()?;
            pty::start_pty(&program, &args, move |event| {
                on_event.send(event).map(|_| ()).map_err(|_| ())
            })
        })();
        match started {
            Ok(session) => {
                let stored = temp_file.as_ref().map(|file| file.path().to_path_buf());
                match app_for_task
                    .state::<SessionState>()
                    .install_prepared(&id, session, stored)
                {
                    Ok(response) => {
                        if let Some(file) = temp_file.as_mut() {
                            file.disarm();
                        }
                        Ok(response)
                    }
                    Err((session, _)) => {
                        let _ = session.shutdown();
                        Err("会话已取消".to_string())
                    }
                }
            }
            Err(err) => {
                app_for_task.state::<SessionState>().abort(&id);
                Err(err)
            }
        }
    })
    .await;

    match joined {
        Ok(result) => {
            // The blocking task already installed or aborted the reservation.
            reservation.armed = false;
            result
        }
        Err(_) => Err("启动会话失败".to_string()),
    }
}

/// Write raw input to the session. The bytes are not stored.
///
/// 写入可能阻塞，因此放在 `spawn_blocking` 中；写锁只在该闭包内持有。
#[tauri::command]
pub async fn write_session(app: tauri::AppHandle, id: String, data: String) -> Result<(), String> {
    {
        let auth = app.state::<crate::auth::AuthState>();
        crate::auth::require_access(&auth, auth.lock_enabled())
    }
        .map_err(|err| err.to_string())?;
    let bytes = data.into_bytes();
    tauri::async_runtime::spawn_blocking(move || app.state::<SessionState>().write(&id, &bytes))
        .await
        .map_err(|_| "写入会话失败".to_string())?
}

/// Resize the PTY. Zero is rejected; very large values are clamped.
///
/// 调整大小可能阻塞，因此放在 `spawn_blocking` 中；锁不跨 `.await`。
#[tauri::command]
pub async fn resize_session(
    app: tauri::AppHandle,
    id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    {
        let auth = app.state::<crate::auth::AuthState>();
        crate::auth::require_access(&auth, auth.lock_enabled())
    }
        .map_err(|err| err.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<SessionState>().resize(&id, cols, rows)
    })
    .await
    .map_err(|_| "调整终端大小失败".to_string())?
}

/// Kill the child and wait until it has been reaped.
///
/// 回收放在 `spawn_blocking` 中。线程的 `JoinError` 转成错误，不 panic。
#[tauri::command]
pub async fn close_session(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let session = app.state::<SessionState>().take(&id)?;
    let temp = app.state::<SessionState>().take_temp(&id);
    if session.is_none() && temp.is_none() {
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let result = if let Some(session) = session {
            session.shutdown()
        } else {
            Ok(())
        };
        if let Some(path) = temp {
            remove_temp_config(&path);
        }
        result
    })
    .await
    .map_err(|_| "结束会话失败".to_string())??;
    Ok(())
}

fn remove_temp_config(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    struct ShutdownGuard<'a>(&'a SessionState);

    impl Drop for ShutdownGuard<'_> {
        fn drop(&mut self) {
            self.0.shutdown_blocking();
        }
    }

    #[test]
    fn abort_drops_a_reservation_and_missing_take_is_empty() {
        let state = SessionState::new();
        let first = state.begin().unwrap();
        let second = state.begin().unwrap();
        assert_ne!(first, second);
        state.abort(&first);
        assert_eq!(state.pending_count(), 1);
        assert!(state.take("missing").unwrap().is_none());
        state.shutdown_blocking();
        assert_eq!(state.pending_count(), 0);
        assert_eq!(state.live_count(), 0);
    }

    #[test]
    fn three_begins_succeed_with_distinct_ids() {
        let state = SessionState::new();
        let first = state.begin().unwrap();
        let second = state.begin().unwrap();
        let third = state.begin().unwrap();
        assert_ne!(first, second);
        assert_ne!(first, third);
        assert_ne!(second, third);
        state.shutdown_blocking();
        assert_eq!(state.pending_count(), 0);
        assert_eq!(state.live_count(), 0);
    }

    #[test]
    fn ninth_begin_returns_the_tab_limit_error() {
        let state = SessionState::new();
        let mut ids = Vec::new();
        for _ in 0..MAX_TABS {
            ids.push(state.begin().unwrap());
        }
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            MAX_TABS
        );
        assert_eq!(state.begin().unwrap_err(), "最多 8 个标签");
        state.shutdown_blocking();
    }

    #[test]
    fn shutdown_clears_a_reservation_so_another_session_can_start() {
        let state = SessionState::new();
        let first = state.begin().unwrap();
        state.shutdown_blocking();
        assert!(state.take("session-1").unwrap().is_none());
        assert_eq!(state.pending_count(), 0);
        let next = state.begin().unwrap();
        assert_ne!(first, next);
        let another = state.begin().unwrap();
        assert_ne!(next, another);
        state.shutdown_blocking();
    }

    #[test]
    fn closing_the_middle_of_three_sessions_keeps_the_others() {
        let state = SessionState::new();
        let _guard = ShutdownGuard(&state);
        let first = spawn_resident(&state);
        let middle = spawn_resident(&state);
        let third = spawn_resident(&state);
        assert_eq!(state.live_count(), 3);

        let started = Instant::now();
        let session = state
            .take(&middle.id)
            .unwrap()
            .expect("middle session is installed");
        session.shutdown().expect("middle shutdown");
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "closing the middle session took {elapsed:?}"
        );
        assert!(
            middle.reaped.load(Ordering::SeqCst),
            "closed session was not reaped"
        );
        assert_eq!(state.live_count(), 2);
        assert_eq!(
            state.write(&middle.id, b"echo GONE\n").unwrap_err(),
            "会话不存在"
        );
        state.resize(&first.id, 100, 30).expect("resize first");
        assert!(state.resize(&middle.id, 80, 24).is_err());

        expect_marker(&state, &first, "PONG-1");
        expect_marker(&state, &third, "PONG-3");
    }

    #[test]
    fn shutdown_blocking_reaps_all_sessions_within_the_deadline() {
        let state = SessionState::new();
        let _guard = ShutdownGuard(&state);
        let sessions = vec![
            spawn_resident(&state),
            spawn_resident(&state),
            spawn_resident(&state),
        ];
        assert_eq!(state.live_count(), 3);

        let started = Instant::now();
        state.shutdown_blocking();
        let elapsed = started.elapsed();
        assert!(
            elapsed <= SHUTDOWN_DEADLINE,
            "shutdown_blocking took {elapsed:?}"
        );
        assert_eq!(state.live_count(), 0);
        assert_eq!(state.pending_count(), 0);
        for session in &sessions {
            assert!(
                session.reaped.load(Ordering::SeqCst),
                "session {} was not reaped",
                session.id
            );
            assert_eq!(state.write(&session.id, b"x").unwrap_err(), "会话不存在");
        }
    }

    struct Resident {
        id: String,
        rx: mpsc::Receiver<SessionEvent>,
        reaped: Arc<std::sync::atomic::AtomicBool>,
    }

    fn spawn_resident(state: &SessionState) -> Resident {
        let id = state.begin().expect("begin");
        let (tx, rx) = mpsc::channel();
        let (program, args) = resident_program();
        let session = pty::start_pty(&program, &args, move |event| tx.send(event).map_err(|_| ()))
            .expect("start_pty");
        let reaped = Arc::clone(&session.reaped);
        state.install(&id, session).unwrap_or_else(|session| {
            let _ = session.shutdown();
            panic!("install {id}");
        });
        Resident { id, rx, reaped }
    }

    fn resident_program() -> (std::path::PathBuf, Vec<String>) {
        if cfg!(windows) {
            let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
            (
                std::path::PathBuf::from(root)
                    .join("System32")
                    .join("cmd.exe"),
                Vec::new(),
            )
        } else {
            (std::path::PathBuf::from("/bin/sh"), Vec::new())
        }
    }

    fn expect_marker(state: &SessionState, resident: &Resident, marker: &str) {
        let mut output = Vec::new();
        let mut answered = false;
        let prime = Instant::now() + Duration::from_millis(400);
        while Instant::now() < prime {
            pump(state, resident, &mut output, &mut answered);
            thread::sleep(Duration::from_millis(20));
        }
        let line = if cfg!(windows) {
            format!("echo {marker}\r\n")
        } else {
            format!("printf '{marker}\\n'\n")
        };
        state
            .write(&resident.id, line.as_bytes())
            .unwrap_or_else(|err| panic!("write {marker}: {err}"));
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            pump(state, resident, &mut output, &mut answered);
            if String::from_utf8_lossy(&output).contains(marker) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "session {} did not show {marker}, output={:?}",
            resident.id,
            String::from_utf8_lossy(&output)
        );
    }

    fn pump(state: &SessionState, resident: &Resident, output: &mut Vec<u8>, answered: &mut bool) {
        while let Ok(event) = resident.rx.try_recv() {
            match event {
                SessionEvent::Data { bytes } => output.extend(bytes),
                SessionEvent::Exit { code } => {
                    panic!("session {} exited early: {code:?}", resident.id);
                }
            }
        }
        const QUERY: &[u8] = b"\x1b[6n";
        if !*answered && output.windows(QUERY.len()).any(|window| window == QUERY) {
            *answered = true;
            state
                .write(&resident.id, b"\x1b[1;1R")
                .expect("cursor answer");
        }
    }

    #[test]
    fn write_rejects_a_single_payload_over_64_kib() {
        let state = SessionState::new();
        let over = vec![1u8; MAX_WRITE_BYTES + 1];
        assert_eq!(
            state.write("session-1", &over).unwrap_err(),
            "单次写入不能超过 64 KiB"
        );
        let exact = vec![1u8; MAX_WRITE_BYTES];
        assert_eq!(state.write("session-1", &exact).unwrap_err(), "会话不存在");
    }

    #[test]
    fn session_event_keeps_partial_utf8_bytes() {
        let event = SessionEvent::Data {
            bytes: vec![0xE4, 0xBD],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "data");
        assert_eq!(json["bytes"], serde_json::json!([0xE4, 0xBD]));

        let exit = SessionEvent::Exit { code: Some(0) };
        let json = serde_json::to_value(&exit).unwrap();
        assert_eq!(json["kind"], "exit");
        assert_eq!(json["code"], 0);

        let unknown = SessionEvent::Exit { code: None };
        let json = serde_json::to_value(&unknown).unwrap();
        assert!(json["code"].is_null());
    }

    #[test]
    fn shutdown_removes_a_temp_ssh_config() {
        let state = SessionState::new();
        let path = std::env::temp_dir().join(format!(
            "my-terminal-ssh-test-{}-shutdown.conf",
            std::process::id()
        ));
        std::fs::write(&path, "Match all\n").unwrap();
        state
            .lock()
            .temp_configs
            .insert("session-x".to_string(), path.clone());
        state.shutdown_blocking();
        assert!(!path.exists());
    }
}

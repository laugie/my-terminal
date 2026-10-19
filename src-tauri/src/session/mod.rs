mod args;
mod binary;
mod io;
mod pty;
mod size;

use std::sync::{Arc, Mutex, PoisonError};

use pty::LiveSession;
use tauri::Manager;

pub use args::{build_ssh_args, SshTarget};
pub use binary::{resolve_ssh_binary, resolve_ssh_binary_with, OsKind, SSH_NOT_FOUND};
pub use size::{normalize_pty_size, MAX_PTY_COLS, MAX_PTY_ROWS};

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

struct Inner {
    generation: u64,
    slot: Slot,
}

enum Slot {
    Idle,
    Starting,
    Active(LiveSession),
}

/// Process-wide single-session table.
pub struct SessionState {
    inner: Mutex<Inner>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                generation: 0,
                slot: Slot::Idle,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock still has to be able to stop the child.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(&self) -> Result<u64, String> {
        let mut guard = self.lock();
        if !matches!(guard.slot, Slot::Idle) {
            return Err("已有会话正在进行".to_string());
        }
        guard.generation = guard.generation.wrapping_add(1);
        guard.slot = Slot::Starting;
        Ok(guard.generation)
    }

    fn abort(&self, generation: u64) {
        let mut guard = self.lock();
        if guard.generation == generation && matches!(guard.slot, Slot::Starting) {
            guard.slot = Slot::Idle;
        }
    }

    fn install(
        &self,
        generation: u64,
        session: LiveSession,
    ) -> Result<OpenSessionResponse, LiveSession> {
        let mut guard = self.lock();
        if guard.generation != generation || !matches!(guard.slot, Slot::Starting) {
            return Err(session);
        }
        let id = session.id.clone();
        guard.slot = Slot::Active(session);
        Ok(OpenSessionResponse { id })
    }

    fn take(&self, id: &str) -> Result<Option<LiveSession>, String> {
        let mut guard = self.lock();
        match &guard.slot {
            Slot::Active(session) if session.id == id => {
                guard.generation = guard.generation.wrapping_add(1);
                match std::mem::replace(&mut guard.slot, Slot::Idle) {
                    Slot::Active(session) => Ok(Some(session)),
                    _ => Ok(None),
                }
            }
            Slot::Active(_) => Err("会话不存在".to_string()),
            _ => Ok(None),
        }
    }

    fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
        let writer = {
            let guard = self.lock();
            match &guard.slot {
                Slot::Active(session) if session.id == id => Arc::clone(&session.writer),
                _ => return Err("会话不存在".to_string()),
            }
        };
        let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
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
            match &guard.slot {
                Slot::Active(session) if session.id == id => Arc::clone(&session.master),
                _ => return Err("会话不存在".to_string()),
            }
        };
        let master = master.lock().unwrap_or_else(PoisonError::into_inner);
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

    /// Kill the live child, if any, and wait until it has been reaped.
    ///
    /// Also invalidates an in-flight `open_session` so it will not install
    /// a process after the window has closed.
    pub fn shutdown_blocking(&self) {
        let session = {
            let mut guard = self.lock();
            guard.generation = guard.generation.wrapping_add(1);
            match std::mem::replace(&mut guard.slot, Slot::Idle) {
                Slot::Active(session) => Some(session),
                _ => None,
            }
        };
        if let Some(session) = session {
            let _ = session.shutdown();
        }
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

struct Reservation {
    app: tauri::AppHandle,
    generation: u64,
    armed: bool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.armed {
            self.app.state::<SessionState>().abort(self.generation);
        }
    }
}

/// Start the single system ssh session.
///
/// 碰外部进程的命令一律 async。创建 PTY 与等待子进程都在阻塞线程里完成，
/// 状态锁不跨 `.await` 持有。`JoinError` 转成错误，不 panic。
#[tauri::command]
pub async fn open_session(
    app: tauri::AppHandle,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    on_event: tauri::ipc::Channel<SessionEvent>,
) -> Result<OpenSessionResponse, String> {
    let args = build_ssh_args(&SshTarget { host, user, port })?;
    let generation = app.state::<SessionState>().begin()?;
    let mut reservation = Reservation {
        app: app.clone(),
        generation,
        armed: true,
    };
    let app_for_task = app.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let started = (|| {
            let program = resolve_ssh_binary()?;
            pty::start_pty(&program, &args, on_event)
        })();
        match started {
            Ok(session) => match app_for_task
                .state::<SessionState>()
                .install(generation, session)
            {
                Ok(response) => Ok(response),
                Err(session) => {
                    let _ = session.shutdown();
                    Err("会话已取消".to_string())
                }
            },
            Err(err) => {
                app_for_task.state::<SessionState>().abort(generation);
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
    if let Some(session) = session {
        tauri::async_runtime::spawn_blocking(move || session.shutdown())
            .await
            .map_err(|_| "结束会话失败".to_string())??;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_begin_is_rejected_until_abort() {
        let state = SessionState::new();
        let first = state.begin().unwrap();
        assert_eq!(state.begin().unwrap_err(), "已有会话正在进行");
        state.abort(first);
        let second = state.begin().unwrap();
        assert_ne!(first, second);
        assert!(state.take("missing").unwrap().is_none());
    }

    #[test]
    fn shutdown_clears_a_reservation_so_another_session_can_start() {
        let state = SessionState::new();
        let generation = state.begin().unwrap();
        state.shutdown_blocking();
        assert!(state.take("session-1").unwrap().is_none());
        let next = state.begin().unwrap();
        assert_ne!(generation, next);
        assert_eq!(state.begin().unwrap_err(), "已有会话正在进行");
        state.abort(next);
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
}

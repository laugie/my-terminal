//! File transfer. Each connection is one system ssh process running the
//! `sftp` subsystem on the same route as the tab (`-s ... sftp`), so jump
//! hosts, command relays, keys and `~/.ssh/config` behave as they do for the
//! terminal. The client speaks SFTP version 3 over the child's pipes.
//!
//! The connection uses `BatchMode=yes` like the remote shell script: a host
//! that only takes typed credentials fails at once with a message.

pub mod client;
pub mod local;
pub mod proto;
pub mod transfer;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use tauri::Manager;

use client::{SftpClient, SftpError};
use local::Listing;
use transfer::{FailKind, TransferError};

/// ssh connect (10 s), login and the SFTP `VERSION` reply together.
const START_TIMEOUT: Duration = Duration::from_secs(40);
/// One per session tab is enough.
const MAX_CONNECTIONS: usize = 8;
const MAX_TRANSFERS: usize = 64;
/// Most progress events per second and transfer, about eight.
const PROGRESS_EVERY: Duration = Duration::from_millis(120);
/// Bytes of ssh stderr kept to explain a failed start.
const STDERR_CAP: usize = 8 * 1024;

const NO_CONNECTION: &str = "文件传输连接已关闭";

/// Tab arguments turned into an SFTP run: no tty, no prompts, keepalives so
/// a dead link is noticed, and the `sftp` subsystem after the host.
pub fn sftp_args(mut args: Vec<String>) -> Result<Vec<String>, String> {
    if let Some(first) = args.iter_mut().find(|arg| arg.as_str() == "-t") {
        *first = "-T".to_string();
    }
    let dash = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| "ssh 参数缺少 --".to_string())?;
    let extra = [
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "-s",
    ];
    for (offset, arg) in extra.into_iter().enumerate() {
        args.insert(dash + offset, arg.to_string());
    }
    args.push("sftp".to_string());
    Ok(args)
}

/// Why a connection did not start, from the client error, the ssh exit code
/// and what ssh printed.
pub fn start_error(err: &SftpError, exit_code: Option<i32>, stderr: &str) -> String {
    let detail = crate::hosts::summarize_cli_text(stderr);
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!("：{detail}")
    };
    if stderr.contains("subsystem request failed") {
        return "远端没有开启 SFTP（sshd 未配置 sftp 子系统）".to_string();
    }
    match err {
        SftpError::Protocol(_) if exit_code.is_none() || exit_code == Some(0) => {
            "远端登录时输出了多余文字，SFTP 无法开始。请让 ~/.bashrc 等文件在非交互登录时不输出内容"
                .to_string()
        }
        SftpError::Timeout => format!("连接超时{suffix}"),
        _ if exit_code == Some(255) => {
            format!("无法免交互登录远端，文件传输需要密钥、ssh-agent 或 Tailscale SSH{suffix}")
        }
        _ => format!("无法打开文件传输{suffix}"),
    }
}

struct Connection {
    client: Arc<SftpClient>,
    child: Mutex<Option<Child>>,
    /// Holds any temporary ssh config until the child is gone.
    _plan: crate::topology::SessionPlan,
}

impl Connection {
    fn shutdown(&self) {
        let child = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Transfer {
    connection: String,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Inner {
    /// Bumped by `shutdown_blocking` so an in-flight open cannot install.
    epoch: u64,
    connections: HashMap<String, Arc<Connection>>,
    transfers: HashMap<String, Transfer>,
}

/// Open SFTP connections and running transfers.
#[derive(Default)]
pub struct SftpState {
    inner: Mutex<Inner>,
}

impl SftpState {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn room(&self) -> Result<u64, String> {
        let guard = self.lock();
        if guard.connections.len() >= MAX_CONNECTIONS {
            return Err(format!("最多同时打开 {MAX_CONNECTIONS} 个文件传输"));
        }
        Ok(guard.epoch)
    }

    fn install(&self, epoch: u64, connection: Connection) -> Result<String, String> {
        let mut guard = self.lock();
        if guard.epoch != epoch {
            return Err("文件传输已取消".to_string());
        }
        if guard.connections.len() >= MAX_CONNECTIONS {
            return Err(format!("最多同时打开 {MAX_CONNECTIONS} 个文件传输"));
        }
        let id = next_id("sftp");
        guard.connections.insert(id.clone(), Arc::new(connection));
        Ok(id)
    }

    fn client(&self, id: &str) -> Result<Arc<SftpClient>, String> {
        let guard = self.lock();
        let connection = guard
            .connections
            .get(id)
            .ok_or_else(|| NO_CONNECTION.to_string())?;
        if connection.client.is_closed() {
            return Err("连接已断开".to_string());
        }
        Ok(Arc::clone(&connection.client))
    }

    fn begin_transfer(&self, connection: &str) -> Result<(String, Arc<AtomicBool>), String> {
        let mut guard = self.lock();
        if !guard.connections.contains_key(connection) {
            return Err(NO_CONNECTION.to_string());
        }
        if guard.transfers.len() >= MAX_TRANSFERS {
            return Err("同时进行的传输太多".to_string());
        }
        let id = next_id("transfer");
        let cancel = Arc::new(AtomicBool::new(false));
        guard.transfers.insert(
            id.clone(),
            Transfer {
                connection: connection.to_string(),
                cancel: Arc::clone(&cancel),
            },
        );
        Ok((id, cancel))
    }

    fn end_transfer(&self, id: &str) {
        self.lock().transfers.remove(id);
    }

    fn cancel_transfer(&self, id: &str) {
        if let Some(transfer) = self.lock().transfers.get(id) {
            transfer.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Remove a connection and stop its transfers. The caller ends the child.
    fn take(&self, id: &str) -> Option<Arc<Connection>> {
        let mut guard = self.lock();
        for transfer in guard.transfers.values() {
            if transfer.connection == id {
                transfer.cancel.store(true, Ordering::Relaxed);
            }
        }
        guard.connections.remove(id)
    }

    /// Stop every transfer and end every ssh child. Used when the app exits.
    pub fn shutdown_blocking(&self) {
        let connections = {
            let mut guard = self.lock();
            guard.epoch = guard.epoch.wrapping_add(1);
            for transfer in guard.transfers.values() {
                transfer.cancel.store(true, Ordering::Relaxed);
            }
            std::mem::take(&mut guard.connections)
        };
        for connection in connections.into_values() {
            connection.shutdown();
        }
    }
}

fn next_id(prefix: &str) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!("{prefix}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Start ssh with the sftp subsystem and wait for the SFTP handshake.
fn connect(
    program: &Path,
    args: &[String],
    plan: crate::topology::SessionPlan,
) -> Result<Connection, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::remote_shell::hide_console(&mut command);
    let mut child = command.spawn().map_err(|_| "启动 ssh 失败".to_string())?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("启动 ssh 失败".to_string());
    };
    let stderr = keep_stderr(child.stderr.take());
    match SftpClient::start(stdout, stdin, START_TIMEOUT) {
        Ok(client) => Ok(Connection {
            client,
            child: Mutex::new(Some(child)),
            _plan: plan,
        }),
        Err(err) => {
            let code = wait_for_exit(&mut child, Duration::from_secs(2));
            // Let the stderr thread take what ssh printed last.
            thread::sleep(Duration::from_millis(100));
            let text = {
                let buf = stderr.lock().unwrap_or_else(PoisonError::into_inner);
                String::from_utf8_lossy(&buf).into_owned()
            };
            Err(start_error(&err, code, &text))
        }
    }
}

/// The exit code, or `None` after killing a child that did not exit in time.
fn wait_for_exit(child: &mut Child, limit: Duration) -> Option<i32> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) if started.elapsed() < limit => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Read ssh's stderr for the whole run, keeping only the last few KiB.
fn keep_stderr(pipe: Option<impl Read + Send + 'static>) -> Arc<Mutex<Vec<u8>>> {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let Some(mut pipe) = pipe else {
        return buf;
    };
    let sink = Arc::clone(&buf);
    let _ = thread::Builder::new()
        .name("sftp-stderr".to_string())
        .spawn(move || {
            let mut chunk = [0u8; 1024];
            while let Ok(read) = pipe.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                let mut buf = sink.lock().unwrap_or_else(PoisonError::into_inner);
                buf.extend_from_slice(&chunk[..read]);
                if buf.len() > STDERR_CAP {
                    let extra = buf.len() - STDERR_CAP;
                    buf.drain(..extra);
                }
            }
        });
    buf
}

fn check_access(app: &tauri::AppHandle) -> Result<(), String> {
    let auth = app.state::<crate::auth::AuthState>();
    crate::auth::require_access(&auth, auth.lock_enabled()).map_err(|err| err.to_string())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpOpened {
    pub id: String,
    /// The remote home folder, where the remote pane starts.
    pub home: String,
}

/// Open an SFTP connection for a topology device or a direct host, on the
/// same route as its tab.
#[tauri::command]
pub async fn sftp_open(
    app: tauri::AppHandle,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    device_name: Option<String>,
    connection_name: Option<String>,
) -> Result<SftpOpened, String> {
    check_access(&app)?;
    let planned =
        crate::session::plan_session(&app, host, user, port, device_name, connection_name).await?;
    let epoch = app.state::<SftpState>().room()?;
    let app_for_task = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let program = crate::session::resolve_ssh_binary()?;
        let args = sftp_args(planned.args.clone())?;
        let connection = connect(&program, &args, planned)?;
        let home = connection
            .client
            .realpath(".")
            .map_err(|err| format!("无法读取远端主目录：{err}"))?;
        let id = app_for_task
            .state::<SftpState>()
            .install(epoch, connection)?;
        Ok(SftpOpened { id, home })
    })
    .await
    .map_err(|_| "打开文件传输失败".to_string())?
}

#[tauri::command]
pub async fn sftp_list(app: tauri::AppHandle, id: String, path: String) -> Result<Listing, String> {
    check_access(&app)?;
    let client = app.state::<SftpState>().client(&id)?;
    tauri::async_runtime::spawn_blocking(move || local::list_remote(&client, &path))
        .await
        .map_err(|_| "读取远端文件夹失败".to_string())?
}

/// End the ssh child and stop the connection's transfers.
#[tauri::command]
pub async fn sftp_close(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let Some(connection) = app.state::<SftpState>().take(&id) else {
        return Ok(());
    };
    tauri::async_runtime::spawn_blocking(move || connection.shutdown())
        .await
        .map_err(|_| "关闭文件传输失败".to_string())
}

/// The folder the local pane opens in: Downloads, else the home folder.
#[tauri::command]
pub async fn local_default_dir(app: tauri::AppHandle) -> Result<String, String> {
    check_access(&app)?;
    let paths = app.path();
    let dir = paths
        .download_dir()
        .ok()
        .filter(|dir| dir.is_dir())
        .or_else(|| paths.home_dir().ok())
        .ok_or_else(|| "找不到下载文件夹".to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn local_list(app: tauri::AppHandle, path: String) -> Result<Listing, String> {
    check_access(&app)?;
    tauri::async_runtime::spawn_blocking(move || local::list_local(&path))
        .await
        .map_err(|_| "读取本地文件夹失败".to_string())?
}

/// Progress of one transfer, delivered on the channel passed to `transfer_start`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransferEvent {
    Progress {
        done: u64,
        total: u64,
    },
    Done {
        done: u64,
        total: u64,
    },
    Failed {
        /// `permission`, `diskFull`, `connection`, `notFound`, `cancelled` or `other`.
        reason: String,
        message: String,
        done: u64,
        total: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Upload,
    Download,
}

/// Copy one file or folder. `upload` sends local `source` into the remote
/// folder `dest_dir`; `download` the other way. Returns the transfer id.
#[tauri::command]
pub async fn transfer_start(
    app: tauri::AppHandle,
    id: String,
    direction: String,
    source: String,
    dest_dir: String,
    on_event: tauri::ipc::Channel<TransferEvent>,
) -> Result<String, String> {
    check_access(&app)?;
    let direction = match direction.as_str() {
        "upload" => Direction::Upload,
        "download" => Direction::Download,
        _ => return Err("未知的传输方向".to_string()),
    };
    if source.is_empty() || dest_dir.is_empty() {
        return Err("路径不能为空".to_string());
    }
    let local = match direction {
        Direction::Upload => PathBuf::from(&source),
        Direction::Download => PathBuf::from(&dest_dir),
    };
    if !local.is_absolute() {
        return Err("本地路径必须是绝对路径".to_string());
    }
    let state = app.state::<SftpState>();
    let client = state.client(&id)?;
    let (transfer_id, cancel) = state.begin_transfer(&id)?;
    let app_for_thread = app.clone();
    let for_thread = transfer_id.clone();
    let spawned = thread::Builder::new()
        .name("sftp-transfer".to_string())
        .spawn(move || {
            let mut latest = (0u64, 0u64);
            let mut sent = Instant::now()
                .checked_sub(PROGRESS_EVERY)
                .unwrap_or_else(Instant::now);
            let result = {
                let mut report = |done: u64, total: u64| {
                    latest = (done, total);
                    if sent.elapsed() >= PROGRESS_EVERY {
                        sent = Instant::now();
                        let _ = on_event.send(TransferEvent::Progress { done, total });
                    }
                };
                match direction {
                    Direction::Upload => transfer::upload(
                        &client,
                        Path::new(&source),
                        &dest_dir,
                        &cancel,
                        &mut report,
                    ),
                    Direction::Download => transfer::download(
                        &client,
                        &source,
                        Path::new(&dest_dir),
                        &cancel,
                        &mut report,
                    ),
                }
            };
            app_for_thread
                .state::<SftpState>()
                .end_transfer(&for_thread);
            let (done, total) = latest;
            let _ = on_event.send(finished(result, done, total));
        });
    if spawned.is_err() {
        state.end_transfer(&transfer_id);
        return Err("无法开始传输".to_string());
    }
    Ok(transfer_id)
}

fn finished(result: Result<(), TransferError>, done: u64, total: u64) -> TransferEvent {
    match result {
        Ok(()) => TransferEvent::Done { done, total },
        Err(err) => TransferEvent::Failed {
            reason: err.kind.as_str().to_string(),
            message: err.message,
            done,
            total,
        },
    }
}

#[tauri::command]
pub async fn transfer_cancel(app: tauri::AppHandle, id: String) -> Result<(), String> {
    app.state::<SftpState>().cancel_transfer(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sftp_args_use_the_subsystem_without_a_tty_or_prompts() {
        let args = sftp_args(
            ["-t", "-l", "ops", "-p", "22", "--", "host-a"]
                .map(str::to_string)
                .to_vec(),
        )
        .unwrap();
        assert_eq!(
            args,
            [
                "-T",
                "-l",
                "ops",
                "-p",
                "22",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-s",
                "--",
                "host-a",
                "sftp",
            ]
        );
        assert!(sftp_args(vec!["host".to_string()]).is_err());
    }

    #[test]
    fn sftp_args_keep_a_generated_config_in_front() {
        let args = sftp_args(
            ["-F", "/tmp/x.conf", "-t", "--", "h"]
                .map(str::to_string)
                .to_vec(),
        )
        .unwrap();
        assert_eq!(&args[..3], ["-F", "/tmp/x.conf", "-T"]);
        assert_eq!(&args[args.len() - 3..], ["--", "h", "sftp"]);
    }

    #[test]
    fn start_errors_explain_the_usual_causes() {
        let lost = SftpError::Lost;
        assert!(
            start_error(&lost, Some(255), "Permission denied (publickey).")
                .starts_with("无法免交互登录远端")
        );
        assert_eq!(
            start_error(&lost, Some(255), "subsystem request failed on channel 0"),
            "远端没有开启 SFTP（sshd 未配置 sftp 子系统）"
        );
        let garbage = SftpError::Protocol("数据包长度 1466986351 无效".to_string());
        assert!(start_error(&garbage, None, "").starts_with("远端登录时输出了多余文字"));
        assert!(start_error(&SftpError::Timeout, None, "").starts_with("连接超时"));
        assert_eq!(start_error(&lost, Some(1), ""), "无法打开文件传输");
    }

    #[test]
    fn transfer_events_serialize_with_a_kind_tag() {
        let event = finished(
            Err(TransferError {
                kind: FailKind::Permission,
                message: "远端权限不足：/x".to_string(),
            }),
            5,
            10,
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "failed");
        assert_eq!(json["reason"], "permission");
        assert_eq!(json["done"], 5);
        let json = serde_json::to_value(finished(Ok(()), 10, 10)).unwrap();
        assert_eq!(json["kind"], "done");
        let json = serde_json::to_value(TransferEvent::Progress { done: 1, total: 2 }).unwrap();
        assert_eq!(json["kind"], "progress");
        assert_eq!(json["total"], 2);
    }

    #[test]
    fn cancelling_a_connection_flags_only_its_transfers() {
        let state = SftpState::new();
        {
            let mut guard = state.lock();
            for (id, connection) in [("t1", "a"), ("t2", "b")] {
                guard.transfers.insert(
                    id.to_string(),
                    Transfer {
                        connection: connection.to_string(),
                        cancel: Arc::new(AtomicBool::new(false)),
                    },
                );
            }
        }
        assert!(state.take("a").is_none());
        let guard = state.lock();
        assert!(guard.transfers["t1"].cancel.load(Ordering::Relaxed));
        assert!(!guard.transfers["t2"].cancel.load(Ordering::Relaxed));
    }
}

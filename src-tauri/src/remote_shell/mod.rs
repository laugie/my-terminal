//! Remote shell colors. The fixed script in `beautify.sh` runs over a separate
//! ssh connection on the same route as the tab (`sh -s -- <mode>`, script on
//! stdin). Nothing from the UI or the topology file is put into the script.
//!
//! The connection uses `BatchMode=yes`: a host that only takes typed
//! credentials fails at once with a message instead of waiting for input.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tauri::Manager;

/// The remote script. `.gitattributes` keeps it LF on a Windows checkout.
pub const SCRIPT: &str = include_str!("beautify.sh");

/// The script may download zsh-syntax-highlighting once (60 s limit there).
const RUN_TIMEOUT: Duration = Duration::from_secs(90);

const OUTCOMES: [&str; 7] = [
    "applied",
    "unchanged",
    "skipped",
    "unsupported",
    "restored",
    "nothing",
    "failed",
];

/// Longest text kept from one report line, and most lines of each kind.
const LINE_CHARS: usize = 300;
const MAX_LINES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// After a connection. Skipped on a host that was restored.
    Auto,
    /// The settings button. Clears a previous restore.
    Apply,
    Restore,
}

impl Mode {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "auto" => Ok(Mode::Auto),
            "apply" => Ok(Mode::Apply),
            "restore" => Ok(Mode::Restore),
            _ => Err("未知的远端 Shell 操作".to_string()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Apply => "apply",
            Mode::Restore => "restore",
        }
    }
}

/// What the script reported. Paths use `~` for the remote home.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellReport {
    pub outcome: String,
    pub files: Vec<String>,
    pub backups: Vec<String>,
    pub notes: Vec<String>,
}

/// Read the `MYTERM-*` lines. None without a known `MYTERM-RESULT`.
pub fn parse_report(stdout: &str) -> Option<ShellReport> {
    let mut report = ShellReport::default();
    let mut done = false;
    for line in stdout.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("MYTERM-RESULT ") {
            let outcome = rest.trim();
            if OUTCOMES.contains(&outcome) {
                report.outcome = outcome.to_string();
                done = true;
            }
        } else if let Some(rest) = line.strip_prefix("MYTERM-FILE ") {
            push_line(&mut report.files, rest);
        } else if let Some(rest) = line.strip_prefix("MYTERM-BACKUP ") {
            push_line(&mut report.backups, rest);
        } else if let Some(rest) = line.strip_prefix("MYTERM-NOTE ") {
            push_line(&mut report.notes, rest);
        }
    }
    done.then_some(report)
}

fn push_line(list: &mut Vec<String>, text: &str) {
    if list.len() >= MAX_LINES {
        return;
    }
    let cleaned: String = text
        .chars()
        .filter(|ch| !ch.is_control())
        .take(LINE_CHARS)
        .collect();
    let cleaned = cleaned.trim();
    if !cleaned.is_empty() {
        list.push(cleaned.to_string());
    }
}

/// Turn the tab's ssh arguments into a script run: no tty, no prompts, and
/// the remote command `sh -s -- <mode>` after the host.
pub fn remote_args(mut args: Vec<String>, mode: Mode) -> Result<Vec<String>, String> {
    if let Some(first) = args.iter_mut().find(|arg| arg.as_str() == "-t") {
        *first = "-T".to_string();
    }
    let dash = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| "ssh 参数缺少 --".to_string())?;
    for (offset, arg) in ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]
        .into_iter()
        .enumerate()
    {
        args.insert(dash + offset, arg.to_string());
    }
    args.extend(["sh", "-s", "--", mode.as_str()].map(str::to_string));
    Ok(args)
}

/// Run ssh with the script on stdin and wait for the report.
pub fn run_script(program: &Path, args: &[String], timeout: Duration) -> Result<ShellReport, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command.spawn().map_err(|_| "启动 ssh 失败".to_string())?;

    let script = SCRIPT.replace("\r\n", "\n");
    if let Some(mut stdin) = child.stdin.take() {
        thread::spawn(move || {
            let _ = stdin.write_all(script.as_bytes());
        });
    }
    let stdout = pipe_reader(child.stdout.take());
    let stderr = pipe_reader(child.stderr.take());

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // A relay's proxy process can hold the pipes a little longer; do not wait on it forever.
    let stdout = stdout.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let stderr = stderr.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let Some(status) = status else {
        return Err("远端 Shell 美化超时".to_string());
    };
    if let Some(report) = parse_report(&String::from_utf8_lossy(&stdout)) {
        return Ok(report);
    }
    let detail = crate::hosts::summarize_cli_text(&String::from_utf8_lossy(&stderr));
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!("：{detail}")
    };
    if status.code() == Some(255) {
        Err(format!("无法免交互登录远端，自动美化需要密钥或 ssh-agent{detail}"))
    } else {
        Err(format!("远端 Shell 美化没有完成{detail}"))
    }
}

fn pipe_reader(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW`: do not flash a console when the GUI starts ssh.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// Run the script for a topology device or a direct host, on the same route
/// as its tab. `mode` is `auto`, `apply` or `restore`.
#[tauri::command]
pub async fn remote_shell_run(
    app: tauri::AppHandle,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    device_name: Option<String>,
    connection_name: Option<String>,
    mode: String,
) -> Result<ShellReport, String> {
    {
        let auth = app.state::<crate::auth::AuthState>();
        crate::auth::require_access(&auth, auth.lock_enabled())
    }
    .map_err(|err| err.to_string())?;
    let mode = Mode::parse(&mode)?;
    let planned =
        crate::session::plan_session(&app, host, user, port, device_name, connection_name).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let program = crate::session::resolve_ssh_binary()?;
        let args = remote_args(planned.args.clone(), mode)?;
        // `planned` owns any temporary ssh config until the run ends.
        let result = run_script(&program, &args, RUN_TIMEOUT);
        drop(planned);
        result
    })
    .await
    .map_err(|_| "远端 Shell 美化没有完成".to_string())?
}

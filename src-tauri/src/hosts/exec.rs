use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Fixed argv for `tailscale status --json`. User input is never appended.
pub const TAILSCALE_STATUS_ARGS: &[&str] = &["status", "--json"];

/// Production timeout. Tests exercise the same runner with a shorter duration.
pub const TAILSCALE_STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Official macOS app binary. Checked after PATH and the usual CLI prefixes.
pub const MACOS_TAILSCALE_PATH: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// Wrapper installed by the Tailscale menu's "Install CLI" action.
pub const USR_LOCAL_TAILSCALE_PATH: &str = "/usr/local/bin/tailscale";

/// Homebrew prefix on Apple silicon.
pub const HOMEBREW_TAILSCALE_PATH: &str = "/opt/homebrew/bin/tailscale";

/// Generic Windows location. The expanded Program Files directory is not shown.
pub const WINDOWS_TAILSCALE_DISPLAY: &str = r"%ProgramFiles%\Tailscale\tailscale.exe";

/// Generic places named in the "command not found" message. PATH is not expanded.
pub const TAILSCALE_SEARCH_DISPLAY: &[&str] = &[
    "PATH",
    USR_LOCAL_TAILSCALE_PATH,
    HOMEBREW_TAILSCALE_PATH,
    MACOS_TAILSCALE_PATH,
    WINDOWS_TAILSCALE_DISPLAY,
];

/// Maximum characters kept from stderr or other CLI text shown in the UI.
const CLI_TEXT_SUMMARY_CHARS: usize = 200;

/// Outcome of one status invocation. The UI maps this onto the list response.
#[derive(Debug)]
pub enum RunOutput {
    NotInstalled,
    TimedOut {
        program: String,
    },
    Failed {
        program: String,
        detail: String,
    },
    Finished {
        code: Option<i32>,
        stdout: String,
        stderr: String,
        program: String,
    },
}

/// Locate the Tailscale CLI.
///
/// Order: an existing PATH hit, `/usr/local/bin/tailscale`,
/// `/opt/homebrew/bin/tailscale`, the macOS app bundle, then
/// `%ProgramFiles%\Tailscale\tailscale.exe`. `exists` is injected so tests can
/// simulate each location without touching the real filesystem.
pub fn locate_tailscale_cli(
    path_hit: Option<PathBuf>,
    exists: impl Fn(&Path) -> bool,
    program_files: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = path_hit {
        return Some(path);
    }
    for candidate in [
        USR_LOCAL_TAILSCALE_PATH,
        HOMEBREW_TAILSCALE_PATH,
        MACOS_TAILSCALE_PATH,
    ] {
        let path = Path::new(candidate);
        if exists(path) {
            return Some(path.to_path_buf());
        }
    }
    if let Some(dir) = program_files {
        let windows = dir.join("Tailscale").join("tailscale.exe");
        if exists(&windows) {
            return Some(windows);
        }
    }
    None
}

fn path_hit_from_env() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in ["tailscale", "tailscale.exe"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Run `tailscale status --json` once, directly, with a 5 second timeout.
pub fn tailscale_status() -> RunOutput {
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    match locate_tailscale_cli(
        path_hit_from_env(),
        |path| path.is_file(),
        program_files.as_deref(),
    ) {
        None => RunOutput::NotInstalled,
        Some(path) => run_command(&path, TAILSCALE_STATUS_ARGS, TAILSCALE_STATUS_TIMEOUT),
    }
}

pub(crate) fn run_command(program: &Path, args: &[&str], timeout: Duration) -> RunOutput {
    let mut command = Command::new(program);
    command.args(args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // The app bundle binary stays in GUI mode unless TERM or SHLVL is set.
        // Finder sets neither. TERM=dumb selects CLI mode without a shell and
        // without depending on the parent environment. Do not set SHLVL.
        .env("TERM", "dumb");
    hide_console(&mut command);

    let program_label = public_program(program);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return RunOutput::NotInstalled,
        Err(err) => {
            return RunOutput::Failed {
                program: program_label,
                detail: err.to_string(),
            };
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_thread = thread::spawn(move || read_pipe(stdout));
    let stderr_thread = thread::spawn(move || read_pipe(stderr));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return RunOutput::TimedOut {
                    program: program_label,
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return RunOutput::Failed {
                    program: program_label,
                    detail: err.to_string(),
                };
            }
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    RunOutput::Finished {
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        program: program_label,
    }
}

fn public_program(path: &Path) -> String {
    redact_home(&path.to_string_lossy())
}

/// Replace the current home directory with `~` and drop control characters.
/// The result is capped at 200 characters and is safe to show in the UI.
pub(crate) fn summarize_cli_text(text: &str) -> String {
    let redacted = redact_home(text);
    let mut cleaned = String::new();
    let mut pending_space = false;
    for ch in redacted.chars() {
        if ch.is_control() {
            pending_space = true;
            continue;
        }
        if pending_space && !cleaned.is_empty() && !cleaned.ends_with(' ') {
            cleaned.push(' ');
        }
        pending_space = false;
        cleaned.push(ch);
    }
    cleaned
        .trim()
        .chars()
        .take(CLI_TEXT_SUMMARY_CHARS)
        .collect()
}

fn redact_home(text: &str) -> String {
    let mut homes = Vec::new();
    for key in ["HOME", "USERPROFILE"] {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        let trimmed = value.trim_end_matches(['/', '\\']).to_string();
        if trimmed.len() <= 1 {
            continue;
        }
        let forward = trimmed.replace('\\', "/");
        if forward != trimmed {
            homes.push(forward);
        }
        homes.push(trimmed);
    }
    homes.sort_by_key(|home| std::cmp::Reverse(home.len()));
    homes.dedup();
    let mut result = text.to_string();
    for home in homes {
        result = result.replace(&home, "~");
    }
    result
}

fn read_pipe(pipe: Option<impl Read>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut buf);
    }
    buf
}

fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW`: do not flash a console when the GUI launches the CLI.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_returns_none_when_nothing_exists() {
        let found = locate_tailscale_cli(None, |_| false, Some(Path::new("/opt/program-files")));
        assert_eq!(found, None);
    }

    #[test]
    fn locate_uses_macos_default_when_path_misses() {
        let found = locate_tailscale_cli(
            None,
            |path| path == Path::new(MACOS_TAILSCALE_PATH),
            Some(Path::new("/opt/program-files")),
        );
        assert_eq!(found, Some(PathBuf::from(MACOS_TAILSCALE_PATH)));
    }

    #[test]
    fn locate_uses_windows_default_when_path_and_macos_miss() {
        let root = Path::new("/opt/program-files");
        let expected = root.join("Tailscale").join("tailscale.exe");
        let found = locate_tailscale_cli(None, |path| path == expected, Some(root));
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn locate_checks_usr_local_then_homebrew_before_the_app_bundle() {
        let usr = Path::new(USR_LOCAL_TAILSCALE_PATH);
        let brew = Path::new(HOMEBREW_TAILSCALE_PATH);
        let app = Path::new(MACOS_TAILSCALE_PATH);
        let found = locate_tailscale_cli(None, |path| path == brew || path == app, None);
        assert_eq!(found.as_deref(), Some(brew));
        let found = locate_tailscale_cli(
            None,
            |path| path == usr || path == brew || path == app,
            None,
        );
        assert_eq!(found.as_deref(), Some(usr));
    }

    #[test]
    fn cli_text_summary_redacts_home_strips_controls_and_limits_length() {
        assert_eq!(summarize_cli_text("ok\u{0001}\nnext"), "ok next");
        assert!(!summarize_cli_text("a\nb").chars().any(|ch| ch.is_control()));
        assert_eq!(summarize_cli_text(&"x".repeat(240)).chars().count(), 200);

        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        let home = home.trim_end_matches(['/', '\\']).to_string();
        if home.len() > 1 {
            let summary = summarize_cli_text(&format!("see {home}/Library/log"));
            assert_eq!(summary, "see ~/Library/log");
            assert!(
                !summary.contains(&home),
                "home directory leaked into the summary"
            );
        }
    }

    #[test]
    fn locate_prefers_path_hit_over_defaults() {
        let from_path = PathBuf::from("/opt/bin/tailscale");
        let found = locate_tailscale_cli(Some(from_path.clone()), |_| true, None);
        assert_eq!(found, Some(from_path));
    }

    #[test]
    fn status_invocation_uses_fixed_args_and_five_second_timeout() {
        assert_eq!(TAILSCALE_STATUS_ARGS, ["status", "--json"]);
        assert_eq!(TAILSCALE_STATUS_TIMEOUT, Duration::from_secs(5));
    }

    #[test]
    fn missing_binary_is_not_installed() {
        let output = run_command(
            Path::new("tailscale-not-present-placeholder"),
            TAILSCALE_STATUS_ARGS,
            Duration::from_secs(2),
        );
        assert!(matches!(output, RunOutput::NotInstalled));
    }

    #[test]
    fn runner_kills_on_timeout() {
        let (program, args) = slow_program();
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let started = Instant::now();
        let output = run_command(&program, &arg_refs, Duration::from_millis(400));
        match output {
            RunOutput::TimedOut { program } => {
                assert!(!program.is_empty(), "timeout names the program")
            }
            other => panic!("expected timeout, got {other:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn runner_captures_stdout_without_a_shell() {
        let output = run_command(Path::new("echo"), &["ok"], Duration::from_secs(2));
        match output {
            RunOutput::Finished { code, stdout, .. } => {
                assert_eq!(code, Some(0));
                assert_eq!(stdout.trim(), "ok");
            }
            other => panic!("expected finished output, got {other:?}"),
        }
    }

    fn slow_program() -> (PathBuf, Vec<String>) {
        if cfg!(windows) {
            (
                PathBuf::from("ping"),
                vec!["-n".into(), "8".into(), "127.0.0.1".into()],
            )
        } else {
            (PathBuf::from("sleep"), vec!["5".into()])
        }
    }
}

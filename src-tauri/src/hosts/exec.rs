use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Fixed argv for `tailscale status --json`. User input is never appended.
pub const TAILSCALE_STATUS_ARGS: &[&str] = &["status", "--json"];

/// Production timeout. Tests exercise the same runner with a shorter duration.
pub const TAILSCALE_STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Official macOS app binary. Checked only after PATH misses.
pub const MACOS_TAILSCALE_PATH: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// Outcome of one status invocation. The UI maps this onto the list response.
#[derive(Debug)]
pub enum RunOutput {
    NotInstalled,
    TimedOut,
    Failed,
    Finished { code: Option<i32>, stdout: String },
}

/// Locate the Tailscale CLI.
///
/// Order: an existing PATH hit, then the macOS app bundle, then
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
    let macos = Path::new(MACOS_TAILSCALE_PATH);
    if exists(macos) {
        return Some(macos.to_path_buf());
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
        .stderr(Stdio::piped());
    hide_console(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return RunOutput::NotInstalled,
        Err(_) => return RunOutput::Failed,
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
                return RunOutput::TimedOut;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return RunOutput::Failed;
            }
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    // stderr is drained so the pipe cannot fill, and then discarded so a
    // local path from the CLI cannot leak into the UI.
    let _stderr = stderr_thread.join().unwrap_or_default();
    RunOutput::Finished {
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
    }
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
        assert!(
            matches!(output, RunOutput::TimedOut),
            "expected timeout, got {output:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn runner_captures_stdout_without_a_shell() {
        let output = run_command(Path::new("echo"), &["ok"], Duration::from_secs(2));
        match output {
            RunOutput::Finished { code, stdout } => {
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

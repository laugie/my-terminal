use std::path::{Path, PathBuf};

pub const SSH_NOT_FOUND: &str = "未找到系统 ssh（Windows 请启用 OpenSSH 客户端）";

/// Which operating system's lookup rules to apply.
/// Injected so tests can cover both layouts on one machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsKind {
    Macos,
    Windows,
}

/// Resolve the system ssh binary.
///
/// macOS uses `/usr/bin/ssh`. Windows prefers
/// `%SystemRoot%\System32\OpenSSH\ssh.exe` and only then an `ssh.exe` found
/// on `PATH`. Anything else is an error; this crate does not ship an ssh
/// implementation.
pub fn resolve_ssh_binary() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        resolve_ssh_binary_with(
            OsKind::Windows,
            Some(&root),
            |path| path.is_file(),
            ssh_exe_on_path(),
        )
    }
    #[cfg(not(windows))]
    {
        resolve_ssh_binary_with(OsKind::Macos, None, |path| path.is_file(), None)
    }
}

/// Lookup with an injected existence probe and an optional PATH hit.
///
/// `path_hit` is consulted only for [`OsKind::Windows`], and only after the
/// System32 copy is missing. The probe is not called for the PATH entry.
pub fn resolve_ssh_binary_with(
    os: OsKind,
    system_root: Option<&Path>,
    mut exists: impl FnMut(&Path) -> bool,
    path_hit: Option<PathBuf>,
) -> Result<PathBuf, String> {
    match os {
        OsKind::Macos => {
            let candidate = PathBuf::from("/usr/bin/ssh");
            if exists(&candidate) {
                Ok(candidate)
            } else {
                Err(SSH_NOT_FOUND.to_string())
            }
        }
        OsKind::Windows => {
            let bundled = windows_openssh_path(system_root);
            if exists(&bundled) {
                Ok(bundled)
            } else if let Some(found) = path_hit {
                Ok(found)
            } else {
                Err(SSH_NOT_FOUND.to_string())
            }
        }
    }
}

fn windows_openssh_path(system_root: Option<&Path>) -> PathBuf {
    let fallback = PathBuf::from(r"C:\Windows");
    let root = system_root.unwrap_or(&fallback);
    root.join("System32").join("OpenSSH").join("ssh.exe")
}

#[cfg(windows)]
fn ssh_exe_on_path() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("ssh.exe");
        if candidate.is_absolute() && candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_uses_usr_bin_ssh_and_ignores_path() {
        let got = resolve_ssh_binary_with(
            OsKind::Macos,
            None,
            |path| path == Path::new("/usr/bin/ssh"),
            Some(PathBuf::from("/custom/ssh")),
        )
        .unwrap();
        assert_eq!(got, PathBuf::from("/usr/bin/ssh"));
    }

    #[test]
    fn macos_missing_binary_is_an_error() {
        let err = resolve_ssh_binary_with(
            OsKind::Macos,
            None,
            |_| false,
            Some(PathBuf::from("/custom/ssh")),
        )
        .unwrap_err();
        assert_eq!(err, SSH_NOT_FOUND);
    }

    #[test]
    fn windows_prefers_system32_over_path() {
        let root = PathBuf::from("/SystemRoot");
        let expected = root.join("System32").join("OpenSSH").join("ssh.exe");
        let mut probed = Vec::new();
        let got = resolve_ssh_binary_with(
            OsKind::Windows,
            Some(&root),
            |path| {
                probed.push(path.to_path_buf());
                path == expected
            },
            Some(PathBuf::from("/Tools/ssh.exe")),
        )
        .unwrap();
        assert_eq!(got, expected);
        assert_eq!(probed, vec![expected]);
    }

    #[test]
    fn windows_falls_back_to_path_then_errors() {
        let root = PathBuf::from("/SystemRoot");
        let fallback = PathBuf::from("/Tools/ssh.exe");
        let got = resolve_ssh_binary_with(
            OsKind::Windows,
            Some(&root),
            |_| false,
            Some(fallback.clone()),
        )
        .unwrap();
        assert_eq!(got, fallback);

        let err =
            resolve_ssh_binary_with(OsKind::Windows, Some(&root), |_| false, None).unwrap_err();
        assert_eq!(err, SSH_NOT_FOUND);
    }

    #[test]
    fn windows_default_root_when_unset() {
        let got = resolve_ssh_binary_with(OsKind::Windows, None, |_| true, None).unwrap();
        let expected = PathBuf::from(r"C:\Windows")
            .join("System32")
            .join("OpenSSH")
            .join("ssh.exe");
        assert_eq!(got, expected);
    }
}

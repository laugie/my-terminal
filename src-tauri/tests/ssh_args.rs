use std::path::Path;

use my_terminal_lib::session::{build_ssh_args, SshTarget};

fn t(host: &str, user: Option<&str>, port: Option<u16>) -> SshTarget {
    SshTarget {
        host: host.into(),
        user: user.map(Into::into),
        port,
    }
}

#[test]
fn plain_target_ends_with_double_dash_and_host() {
    let args = build_ssh_args(&t("host-a.example-tailnet.ts.net", None, None)).unwrap();
    assert_eq!(
        args.last().map(String::as_str),
        Some("host-a.example-tailnet.ts.net")
    );
    assert_eq!(args.iter().rev().nth(1).map(String::as_str), Some("--"));
    assert!(!args.iter().any(|arg| arg == "-p"));
}

#[test]
fn user_and_port_use_separate_flags() {
    let args = build_ssh_args(&t("100.64.0.1", Some("user"), Some(2222))).unwrap();
    let joined = args.join(" ");
    assert!(joined.contains("-l user"), "{joined}");
    assert!(joined.contains("-p 2222"), "{joined}");
    assert!(!joined.contains("user@"), "{joined}");
}

#[test]
fn forces_tty() {
    let args = build_ssh_args(&t("host-a.example-tailnet.ts.net", None, None)).unwrap();
    assert!(args.iter().any(|arg| arg == "-t"));
}

#[test]
fn rejects_option_injection_in_host() {
    for host in [
        "-oProxyCommand=evil",
        "-F/dev/null",
        "host a",
        "host\na",
        "",
        "host;rm",
        "a@b",
    ] {
        assert!(
            build_ssh_args(&t(host, None, None)).is_err(),
            "host {host:?} should be rejected"
        );
    }
}

#[test]
fn rejects_option_injection_in_user() {
    for user in ["-oProxyCommand=evil", "us er", "u\ns", "u@h"] {
        assert!(
            build_ssh_args(&t("host-a.example-tailnet.ts.net", Some(user), None)).is_err(),
            "user {user:?} should be rejected"
        );
    }
}

#[test]
fn never_passes_password_options() {
    let samples = [
        t("host-a.example-tailnet.ts.net", Some("user"), Some(2222)),
        t("100.64.0.1", None, None),
        t("100.64.0.1", Some("user"), None),
    ];
    for target in samples {
        let joined = build_ssh_args(&target).unwrap().join(" ").to_lowercase();
        for word in [
            "password",
            "sshpass",
            "-i ",
            "identityfile",
            "proxycommand",
            "stricthostkeychecking=no",
        ] {
            assert!(!joined.contains(word), "{word} appeared in {joined}");
        }
    }
}

#[test]
fn port_zero_rejected() {
    assert!(build_ssh_args(&t("host-a.example-tailnet.ts.net", None, Some(0))).is_err());
}

const FORBIDDEN_SSH_OPTIONS: &[&str] = &[
    "password",
    "passphrase",
    "identityfile",
    "sshpass",
    "ssh_askpass",
    "stricthostkeychecking",
    "userknownhostsfile",
];

#[test]
fn implementation_sources_do_not_embed_forbidden_ssh_options() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [manifest_dir.join("src"), manifest_dir.join("../src")];
    for root in roots {
        visit(&root, FORBIDDEN_SSH_OPTIONS);
    }
}

/// `password` outside the three exact paths still fails. The three allowed
/// files still fail for every other forbidden word.
#[test]
fn password_outside_allowlisted_files_still_fails() {
    let sample = "type=\"password\"";
    let outside = [
        Path::new("src/main.ts"),
        Path::new("src/auth.ts"),
        Path::new("src/login.tsx"),
        Path::new("src-tauri/src/login.ts"),
        Path::new("src-tauri/src/auth/mod.rs"),
        Path::new("src-tauri/src/auth/throttle.rs"),
        Path::new("src-tauri/src/auth/hash.rs.bak"),
        Path::new("src-tauri/src/hosts/mod.rs"),
        Path::new("other/src/login.ts"),
        Path::new(r"src-tauri\src\auth\store.rs.bak"),
    ];
    for path in outside {
        assert_eq!(
            forbidden_option_in_source(path, sample, FORBIDDEN_SSH_OPTIONS),
            Some("password"),
            "{}",
            path.display()
        );
    }

    let allowed = [
        Path::new("src-tauri/src/auth/hash.rs"),
        Path::new(r"src-tauri\src\auth\store.rs"),
        Path::new("src/login.ts"),
        Path::new("src-tauri/../src/login.ts"),
    ];
    for path in allowed {
        assert_eq!(
            forbidden_option_in_source(path, sample, FORBIDDEN_SSH_OPTIONS),
            None,
            "{}",
            path.display()
        );
        for needle in FORBIDDEN_SSH_OPTIONS
            .iter()
            .copied()
            .filter(|needle| *needle != "password")
        {
            let contents = format!("let note = \"{needle}\";");
            assert_eq!(
                forbidden_option_in_source(path, &contents, FORBIDDEN_SSH_OPTIONS),
                Some(needle),
                "{needle} in {}",
                path.display()
            );
        }
    }
}

/// Reverse of the allowlist: `identityfile` outside the two topology sources
/// still fails, and every other forbidden word still fails inside them.
#[test]
fn identityfile_outside_allowlisted_files_still_fails() {
    let identity = "IdentityFile ~/.ssh/id_example";
    let outside = [
        Path::new("src-tauri/src/session/args.rs"),
        Path::new(r"src-tauri\src\session\args.rs"),
        Path::new("src-tauri/src/topology/mod.rs"),
        Path::new(r"src-tauri\src\topology\validate.rs"),
        Path::new("src-tauri/src/topology/args.rs.bak"),
        Path::new("src/main.ts"),
    ];
    for path in outside {
        assert_eq!(
            forbidden_option_in_source(path, identity, FORBIDDEN_SSH_OPTIONS),
            Some("identityfile"),
            "{}",
            path.display()
        );
    }

    let allowed = [
        Path::new("src-tauri/src/topology/args.rs"),
        Path::new(r"src-tauri\src\topology\args.rs"),
        Path::new("src-tauri/src/topology/parse.rs"),
        Path::new(r"C:\repo\src-tauri\src\topology\parse.rs"),
    ];
    for path in allowed {
        assert_eq!(
            forbidden_option_in_source(path, identity, FORBIDDEN_SSH_OPTIONS),
            None,
            "{}",
            path.display()
        );
    }

    assert_eq!(
        forbidden_option_in_source(
            Path::new(r"src-tauri\src\topology\args.rs"),
            "IdentityFile and password",
            FORBIDDEN_SSH_OPTIONS,
        ),
        Some("password")
    );

    for needle in FORBIDDEN_SSH_OPTIONS
        .iter()
        .copied()
        .filter(|needle| *needle != "identityfile")
    {
        let contents = format!("let note = \"{needle}\";");
        for path in [
            Path::new("src-tauri/src/topology/args.rs"),
            Path::new(r"src-tauri\src\topology\parse.rs"),
        ] {
            assert_eq!(
                forbidden_option_in_source(path, &contents, FORBIDDEN_SSH_OPTIONS),
                Some(needle),
                "{needle} in {}",
                path.display()
            );
        }
    }
}

fn visit(dir: &Path, needles: &[&str]) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|err| panic!("read {}: {err}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            visit(&path, needles);
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        // #16 跳板机需要密钥路径，维护者 2026-10-20 批准：只对 identityfile
        // 放行 src-tauri/src/topology/args.rs 和 src-tauri/src/topology/parse.rs。
        // 其他检查词和其他文件一律不放开。路径比较把反斜杠当成斜杠。
        let hit = forbidden_option_in_source(&path, &text, needles);
        assert!(
            hit.is_none(),
            "{} appeared in {}",
            hit.unwrap_or("forbidden option"),
            path.display()
        );
    }
}

/// Whether `contents` embeds a forbidden ssh option at `path`.
///
/// `identityfile` is ignored only when [`identityfile_allowlisted`] is true.
/// `password` is ignored only when [`password_allowlisted`] is true.
/// Every other needle is reported for every path, including those two lists.
fn forbidden_option_in_source<'a>(
    path: &Path,
    contents: &str,
    needles: &'a [&str],
) -> Option<&'a str> {
    let lower = contents.to_lowercase();
    for needle in needles {
        if !lower.contains(needle) {
            continue;
        }
        if *needle == "identityfile" && identityfile_allowlisted(path) {
            continue;
        }
        if *needle == "password" && password_allowlisted(path) {
            continue;
        }
        return Some(*needle);
    }
    None
}

/// #15 登录锁要在下面三个文件里写出 password 这个词。维护者已批准。
/// 比较前把 `\` 换成 `/`，再取仓库相对路径，必须和下列路径完全相等。
/// 不是 `src/auth/` 的目录前缀，其他禁用词也不在这里放行。
///
/// - `src-tauri/src/auth/hash.rs`：argon2 的 `password_hash` API（`PasswordHash`、`hash_password`、`verify_password`）就叫这个词。
/// - `src-tauri/src/auth/store.rs`：keyring 的 `get_password` / `set_password` 就叫这个词。
/// - `src/login.ts`：HTML 口令框的 `type` 必须是 `password`。
fn password_allowlisted(path: &Path) -> bool {
    const ALLOWED: &[&str] = &[
        "src-tauri/src/auth/hash.rs",
        "src-tauri/src/auth/store.rs",
        "src/login.ts",
    ];
    let normalized = lexical_normalize(&path.to_string_lossy().replace('\\', "/"));
    let relative = strip_repo_root(&normalized);
    ALLOWED.iter().any(|allowed| relative == *allowed)
}

/// Collapse `.` and `..` without reading the filesystem. `src-tauri/../src/login.ts`
/// is the same file as `src/login.ts`; `other/src/login.ts` is not.
fn lexical_normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

/// Drop the repo root from an absolute path. A path that is already relative
/// is unchanged, so `src/login.ts` does not also match `src-tauri/src/login.ts`.
fn strip_repo_root(normalized: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_string_lossy()
        .replace('\\', "/");
    let root = root.trim_end_matches('/');
    normalized
        .strip_prefix(root)
        .unwrap_or(normalized)
        .trim_start_matches('/')
        .to_string()
}

/// #16 跳板机需要密钥路径，维护者 2026-10-20 批准。
/// 只放行这两个文件里的 identityfile。比较前把 `\` 换成 `/`。
fn identityfile_allowlisted(path: &Path) -> bool {
    const ALLOWED: &[&str] = &[
        "src-tauri/src/topology/args.rs",
        "src-tauri/src/topology/parse.rs",
    ];
    let normalized = path.to_string_lossy().replace('\\', "/");
    ALLOWED
        .iter()
        .any(|allowed| normalized == *allowed || normalized.ends_with(&format!("/{allowed}")))
}

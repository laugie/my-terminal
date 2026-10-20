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
/// Every other needle is reported for every path.
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
        return Some(*needle);
    }
    None
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

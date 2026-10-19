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

#[test]
fn implementation_sources_do_not_embed_forbidden_ssh_options() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [manifest_dir.join("src"), manifest_dir.join("../src")];
    let needles = [
        "password",
        "passphrase",
        "identityfile",
        "sshpass",
        "ssh_askpass",
        "stricthostkeychecking",
        "userknownhostsfile",
    ];
    for root in roots {
        visit(&root, &needles);
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
        let lower = text.to_lowercase();
        for needle in needles {
            assert!(
                !lower.contains(needle),
                "{needle} appeared in {}",
                path.display()
            );
        }
    }
}

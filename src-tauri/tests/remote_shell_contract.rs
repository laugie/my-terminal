// #35 失败测试：远端 Shell 美化。
// 脚本内容固定（include_str!），经 ssh 以 `sh -s -- <mode>` 执行；这里不经过 ssh，
// 直接用本机 sh 和临时 HOME 运行同一份脚本。Windows 没有 sh，脚本部分只在 Unix 上跑。
use my_terminal_lib::remote_shell::{parse_report, remote_args, Mode};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[test]
fn remote_args_run_the_script_without_a_tty_or_prompts() {
    let args = remote_args(strings(&["-t", "-l", "me", "-p", "22", "--", "192.0.2.7"]), Mode::Auto).unwrap();
    assert_eq!(
        args,
        strings(&[
            "-T", "-l", "me", "-p", "22", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "--",
            "192.0.2.7", "sh", "-s", "--", "auto"
        ])
    );
}

#[test]
fn remote_args_keep_a_relay_proxy_command() {
    let proxy = "ProxyCommand=/usr/bin/ssh -T -l me -p 22 -- 192.0.2.1 tailscale nc 192.0.2.9 22";
    let args = remote_args(
        strings(&["-t", "-l", "me", "-o", proxy, "--", "192.0.2.9"]),
        Mode::Restore,
    )
    .unwrap();
    assert_eq!(args[0], "-T");
    assert!(args.iter().any(|arg| arg == proxy));
    let dash = args.iter().position(|arg| arg == "--").unwrap();
    assert_eq!(&args[dash - 4..dash], &strings(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"])[..]);
    assert_eq!(&args[dash + 1..], &strings(&["192.0.2.9", "sh", "-s", "--", "restore"])[..]);
}

#[test]
fn remote_args_need_the_host_separator() {
    assert!(remote_args(strings(&["-t", "192.0.2.7"]), Mode::Apply).is_err());
}

#[test]
fn modes_are_a_fixed_list() {
    assert_eq!(Mode::parse("auto").unwrap(), Mode::Auto);
    assert_eq!(Mode::parse("apply").unwrap(), Mode::Apply);
    assert_eq!(Mode::parse("restore").unwrap(), Mode::Restore);
    assert!(Mode::parse("rm -rf").is_err());
    assert!(Mode::parse("").is_err());
}

#[test]
fn report_lines_are_parsed_and_noise_ignored() {
    let out = "Last login: today\r\nMYTERM-NOTE 已下载\r\nMYTERM-FILE ~/.zshrc\nMYTERM-BACKUP ~/.my-terminal/backups/.zshrc.1\nmotd\nMYTERM-RESULT applied\n";
    let report = parse_report(out).unwrap();
    assert_eq!(report.outcome, "applied");
    assert_eq!(report.files, vec!["~/.zshrc"]);
    assert_eq!(report.backups, vec!["~/.my-terminal/backups/.zshrc.1"]);
    assert_eq!(report.notes, vec!["已下载"]);
}

#[test]
fn a_report_needs_a_known_result() {
    assert!(parse_report("MYTERM-FILE ~/.bashrc\n").is_none());
    assert!(parse_report("MYTERM-RESULT exploded\n").is_none());
    let report = parse_report("MYTERM-NOTE a\u{1b}[31mb\nMYTERM-RESULT failed\n").unwrap();
    assert_eq!(report.outcome, "failed");
    assert_eq!(report.notes, vec!["a[31mb"], "控制字符被去掉");
}

#[cfg(unix)]
mod script {
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    use my_terminal_lib::remote_shell::{parse_report, ShellReport, SCRIPT};

    struct Home(PathBuf);

    impl Home {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "my-terminal-shell-{label}-{}-{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn run(home: &Home, shell: &str, mode: &str) -> ShellReport {
        let mut child = Command::new("/bin/sh")
            .args(["-s", "--", mode])
            .env("HOME", &home.0)
            .env("SHELL", shell)
            .env("MY_TERMINAL_NO_FETCH", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(SCRIPT.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{mode}: {stdout} {}", String::from_utf8_lossy(&out.stderr));
        parse_report(&stdout).unwrap_or_else(|| panic!("{mode}: no report in {stdout}"))
    }

    fn read(path: &Path) -> Vec<u8> {
        fs::read(path).unwrap()
    }

    const BASHRC: &str = "export FOO=1\nalias ll='ls -l'";

    #[test]
    fn bash_backup_block_rerun_and_exact_restore() {
        let home = Home::new("bash");
        fs::write(home.file(".bashrc"), BASHRC).unwrap();
        let first = run(&home, "/bin/bash", "auto");
        assert_eq!(first.outcome, "applied");
        assert_eq!(first.files, vec!["~/.bashrc"]);
        let backup = home.0.join(first.backups[0].trim_start_matches("~/"));
        assert_eq!(read(&backup), BASHRC.as_bytes(), "先备份原文件");
        let text = fs::read_to_string(home.file(".bashrc")).unwrap();
        assert!(text.starts_with(BASHRC), "原内容不变");
        assert!(text.contains("# >>> my-terminal shell colors >>>"));
        assert!(home.file(".my-terminal/colors.bash").is_file());

        assert_eq!(run(&home, "/bin/bash", "auto").outcome, "unchanged", "重复执行不再改");

        let restored = run(&home, "/bin/bash", "restore");
        assert_eq!(restored.outcome, "restored");
        assert_eq!(read(&home.file(".bashrc")), BASHRC.as_bytes(), "恢复后逐字节一致");
        assert!(!home.file(".my-terminal/colors.bash").exists());

        assert_eq!(run(&home, "/bin/bash", "auto").outcome, "skipped", "恢复后自动美化不再执行");
        assert_eq!(run(&home, "/bin/bash", "apply").outcome, "applied", "手动美化可以再开");
    }

    #[test]
    fn bash_login_file_that_skips_bashrc_gets_the_block() {
        let home = Home::new("profile");
        fs::write(home.file(".bashrc"), "true\n").unwrap();
        fs::write(home.file(".bash_profile"), "PATH=$PATH\n").unwrap();
        let report = run(&home, "/bin/bash", "auto");
        assert_eq!(report.files, vec!["~/.bashrc", "~/.bash_profile"]);
        run(&home, "/bin/bash", "restore");
        assert_eq!(read(&home.file(".bash_profile")), b"PATH=$PATH\n");
    }

    #[test]
    fn zsh_rc_that_did_not_exist_is_removed_again() {
        let home = Home::new("zsh");
        let report = run(&home, "/bin/zsh", "auto");
        assert_eq!(report.outcome, "applied");
        assert!(home.file(".zshrc").is_file());
        assert!(home.file(".my-terminal/colors.zsh").is_file());
        run(&home, "/bin/zsh", "restore");
        assert!(!home.file(".zshrc").exists());
    }

    #[test]
    fn edits_made_after_beautify_survive_restore() {
        let home = Home::new("edited");
        fs::write(home.file(".zshrc"), "export A=1\n").unwrap();
        run(&home, "/bin/zsh", "auto");
        let mut file = fs::OpenOptions::new().append(true).open(home.file(".zshrc")).unwrap();
        file.write_all(b"export B=2\n").unwrap();
        let report = run(&home, "/bin/zsh", "restore");
        assert_eq!(report.outcome, "restored");
        assert!(!report.notes.is_empty(), "说明只移除了段落");
        assert_eq!(read(&home.file(".zshrc")), b"export A=1\nexport B=2\n");
    }

    #[test]
    fn other_shells_are_left_alone() {
        let home = Home::new("fish");
        assert_eq!(run(&home, "/usr/bin/fish", "auto").outcome, "unsupported");
        assert!(!home.file(".my-terminal/colors.bash").exists());
        assert_eq!(run(&home, "/usr/bin/fish", "restore").outcome, "nothing");
    }

    /// The helper must load cleanly in a real interactive shell.
    fn interactive(shell: &str, home: &Home, command: &str) -> (String, String) {
        let out = Command::new(shell)
            .args(["-i", "-c", command])
            .env("HOME", &home.0)
            .env("TERM", "xterm-256color")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    #[test]
    fn bash_loads_colors_and_keeps_a_colored_prompt() {
        let home = Home::new("bash-i");
        fs::write(home.file(".bashrc"), "PS1='\\[\\033[01;32m\\]\\u\\$ '\n").unwrap();
        run(&home, "/bin/bash", "auto");
        let (stdout, stderr) = interactive("/bin/bash", &home, "alias ls; echo \"CLICOLOR=${CLICOLOR-}\"; echo \"PS1=$PS1\"");
        assert!(!stderr.contains("colors.bash"), "{stderr}");
        assert!(stdout.contains("--color=auto") || stdout.contains("CLICOLOR=1"), "{stdout}");
        assert!(stdout.contains("PS1=\\[\\033[01;32m\\]\\u\\$ "), "已有彩色提示符不改: {stdout}");
    }

    #[test]
    fn zsh_loads_colors_and_a_prompt() {
        if !Path::new("/bin/zsh").exists() {
            return;
        }
        let home = Home::new("zsh-i");
        fs::write(home.file(".zshrc"), "PROMPT='%m%# '\n").unwrap();
        run(&home, "/bin/zsh", "auto");
        let (stdout, stderr) = interactive("/bin/zsh", &home, "alias ls; print -r -- \"CLICOLOR=${CLICOLOR-}\"; print -r -- \"P=$PROMPT\"");
        // Errors inside the prompt function are reported as "(anon):N: ...".
        for marker in ["colors.zsh", "(anon)", "invalid subscript", "parse error"] {
            assert!(!stderr.contains(marker), "{stderr}");
        }
        assert!(stdout.contains("--color=auto") || stdout.contains("CLICOLOR=1"), "{stdout}");
        assert!(stdout.contains("P=%B%F{green}"), "{stdout}");
    }
}

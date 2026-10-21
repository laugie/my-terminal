//! Command relay through a jump host.
//!
//! OpenSSH runs `ProxyCommand` with `/bin/sh -c` (and `cmd.exe` on Windows).
//! That is the only place a command string is handed to a shell. The template
//! must match the allowlist below, and this process still spawns `ssh` itself
//! as an argv array with no shell.

use std::path::Path;

use crate::session::args::{build_ssh_args_with_options, SshOptions};
use crate::session::resolve_ssh_binary;

use super::parse::{Device, Topology};
use super::validate::{index, region_of};

/// Jump and target names used to tell a failed relay apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayEnds {
    pub via_host: String,
    pub via_user: String,
    pub target_host: String,
    pub target_user: String,
}

/// Reject a relay command template.
///
/// Allowed: ASCII letters and digits, space, `.` `_` `/` `:` `=` `@` `+` `-`,
/// and the tokens `%h`, `%p`, and `%%`. `%h` and `%p` must both occur.
pub(crate) fn reject_relay_template(template: &str) -> Result<(), String> {
    if template.is_empty() {
        return Err("中转命令不能为空".to_string());
    }
    if template.starts_with('-') {
        return Err("中转命令不能以 - 开头".to_string());
    }
    let chars: Vec<char> = template.chars().collect();
    let mut index = 0;
    let mut has_host = false;
    let mut has_port = false;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '%' {
            let Some(next) = chars.get(index + 1).copied() else {
                return Err("中转命令包含未知占位符".to_string());
            };
            match next {
                'h' => has_host = true,
                'p' => has_port = true,
                '%' => {}
                other => {
                    return Err(format!("中转命令包含未知占位符 %{other}"));
                }
            }
            index += 2;
            continue;
        }
        if ch == ' ' || allowed_template_char(ch) {
            index += 1;
            continue;
        }
        if ch.is_control() {
            return Err("中转命令不能包含换行或控制字符".to_string());
        }
        return Err(format!("中转命令包含不允许的字符 {ch}"));
    }
    if !has_host || !has_port {
        return Err("中转命令必须同时包含 %h 和 %p".to_string());
    }
    Ok(())
}

fn allowed_template_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '/' | ':' | '=' | '@' | '+' | '-')
}

/// Expand `%h`, `%p`, and `%%` in a template that already passed the allowlist.
///
/// `%%` becomes one `%`. The caller escapes that `%` again when the text is
/// placed in `ProxyCommand`, because ssh treats `%` as its own token.
pub fn expand_relay_template(template: &str, host: &str, port: u16) -> Result<String, String> {
    reject_relay_template(template)?;
    if port == 0 || !shell_safe_host(host) {
        return Err("中转目标无效".to_string());
    }
    let port_text = port.to_string();
    let chars: Vec<char> = template.chars().collect();
    let mut expanded = String::with_capacity(template.len() + host.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '%' {
            match chars[index + 1] {
                'h' => expanded.push_str(host),
                'p' => expanded.push_str(&port_text),
                '%' => expanded.push('%'),
                _ => return Err("中转命令包含未知占位符".to_string()),
            }
            index += 2;
        } else {
            expanded.push(chars[index]);
            index += 1;
        }
    }
    Ok(expanded)
}

/// `ssh_args_for` when `connection_name` is `None`. Otherwise a relay that
/// uses `ProxyCommand` and does not pass `-J`.
pub fn ssh_args_for_connection(
    topology: &Topology,
    device_name: &str,
    connection_name: Option<&str>,
) -> Result<Vec<String>, String> {
    match connection_name {
        None => super::args::ssh_args_for(topology, device_name),
        Some(name) => prepare_relay(topology, device_name, name).map(|(args, _)| args),
    }
}

pub(crate) fn prepare_relay(
    topology: &Topology,
    device_name: &str,
    connection_name: &str,
) -> Result<(Vec<String>, RelayEnds), String> {
    let by_name = index(topology);
    let Some(device) = by_name.get(device_name).copied() else {
        return Err(format!("没有名为「{device_name}」的设备"));
    };
    build_relay(device, &by_name, connection_name).map_err(|err| {
        format!(
            "区域「{}」设备「{}」：{err}",
            region_of(topology, device_name),
            device.name
        )
    })
}

fn build_relay(
    device: &Device,
    by_name: &std::collections::HashMap<&str, &Device>,
    connection_name: &str,
) -> Result<(Vec<String>, RelayEnds), String> {
    let connection = device
        .connections
        .iter()
        .find(|item| item.name == connection_name)
        .ok_or_else(|| format!("没有名为「{connection_name}」的连接"))?;
    if connection.via == device.name {
        return Err("via 不能是自己".to_string());
    }
    let via = by_name
        .get(connection.via.as_str())
        .copied()
        .ok_or_else(|| format!("via 指向不存在的设备「{}」", connection.via))?;
    if via.jump.is_some() || !via.connections.is_empty() {
        return Err("中转跳板机不能再有 jump".to_string());
    }
    if !shell_name_ok(&via.user) || !shell_name_ok(&via.host) {
        return Err("跳板机参数不能用于中转命令".to_string());
    }
    let expanded = expand_relay_template(&connection.command, &device.host, device.port)?;
    let escaped = escape_percents(&expanded);
    let program = proxy_program(&resolve_ssh_binary()?)?;
    let proxy = assemble_proxy(&program, via, &escaped)?;
    let mut args = build_ssh_args_with_options(&SshOptions {
        host: device.host.clone(),
        user: Some(device.user.clone()),
        port: Some(device.port),
        identity_path: device.identity_file.clone(),
        jump: None,
    })?;
    let dash = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| "ssh 参数缺少 --".to_string())?;
    args.insert(dash, format!("ProxyCommand={proxy}"));
    args.insert(dash, "-o".to_string());
    let ends = RelayEnds {
        via_host: via.host.clone(),
        via_user: via.user.clone(),
        target_host: device.host.clone(),
        target_user: device.user.clone(),
    };
    Ok((args, ends))
}

fn assemble_proxy(program: &str, via: &Device, expanded: &str) -> Result<String, String> {
    let mut parts = Vec::with_capacity(12);
    parts.push(program.to_string());
    parts.push("-T".to_string());
    parts.push("-l".to_string());
    parts.push(via.user.clone());
    parts.push("-p".to_string());
    parts.push(via.port.to_string());
    if let Some(key) = via.identity_file.as_deref() {
        if !shell_insert_ok(key) {
            return Err("密钥路径不能用于中转命令".to_string());
        }
        parts.push("-i".to_string());
        parts.push(key.to_string());
    }
    parts.push("--".to_string());
    parts.push(via.host.clone());
    let mut command = parts.join(" ");
    command.push(' ');
    command.push_str(expanded);
    Ok(command)
}

/// Quote a program path only when it contains a space. `"` and `%` are refused.
fn proxy_program(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "ssh 程序路径无效".to_string())?;
    if text
        .chars()
        .any(|ch| ch.is_control() || ch == '"' || ch == '%')
    {
        return Err("ssh 程序路径无效".to_string());
    }
    if text.chars().any(|ch| {
        matches!(
            ch,
            ';' | '|'
                | '&'
                | '$'
                | '`'
                | '\''
                | '<'
                | '>'
                | '('
                | ')'
                | '*'
                | '?'
                | '!'
                | '#'
                | '~'
        )
    }) {
        return Err("ssh 程序路径无效".to_string());
    }
    if text.contains(' ') {
        Ok(format!("\"{text}\""))
    } else {
        Ok(text.to_string())
    }
}

fn escape_percents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '%' {
            out.push('%');
        }
        out.push(ch);
    }
    out
}

fn shell_safe_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('-')
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

/// Jump user and host inserted into `ProxyCommand`. `~` and `\` are refused.
fn shell_name_ok(value: &str) -> bool {
    shell_chars_ok(value, false)
}

/// Key path inserted into `ProxyCommand`. `~` and `\` are allowed only here.
fn shell_insert_ok(value: &str) -> bool {
    shell_chars_ok(value, true)
}

fn shell_chars_ok(value: &str, allow_path_marks: bool) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '.' | '_' | '/' | ':' | '=' | '@' | '+' | '-')
                || (allow_path_marks && matches!(ch, '~' | '\\'))
        })
}

/// Coarse side of an ssh failure, without knowing which host is the jump.
///
/// `"jump"`, `"target"`, or `"unknown"`. Exit codes other than 255 are unknown.
pub fn failure_side(stderr: &str, exit_code: i32) -> &'static str {
    if exit_code != 255 {
        return "unknown";
    }
    if stderr.contains("connect to host ") || stderr.contains("Could not resolve hostname ") {
        return "jump";
    }
    if stderr.contains(": Permission denied") {
        if stderr.contains("UNKNOWN port 65535") {
            return "jump";
        }
        return "target";
    }
    if stderr.contains("Host key verification failed")
        || stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
    {
        return "target";
    }
    "unknown"
}

/// One Chinese line for a relay session, or `None` when ssh did not fail with 255.
pub fn relay_failure_line(stderr: &str, exit_code: i32, ends: &RelayEnds) -> Option<&'static str> {
    if exit_code != 255 {
        return None;
    }
    Some(classify(stderr, ends))
}

fn classify(stderr: &str, ends: &RelayEnds) -> &'static str {
    let jump_connect = format!("connect to host {}", ends.via_host);
    let jump_dns = format!("Could not resolve hostname {}", ends.via_host);
    let jump_denied = format!("{}@{}: Permission denied", ends.via_user, ends.via_host);
    let target_denied = format!(
        "{}@{}: Permission denied",
        ends.target_user, ends.target_host
    );
    let host_key = stderr.contains("Host key verification failed")
        || stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED");
    let mentions_via = contains_bounded(stderr, &ends.via_host);
    let mentions_target = contains_bounded(stderr, &ends.target_host);
    let jump = contains_bounded(stderr, &jump_connect)
        || contains_bounded(stderr, &jump_dns)
        || contains_bounded(stderr, &jump_denied)
        || (host_key && mentions_via && !mentions_target);
    let target = contains_bounded(stderr, &target_denied)
        || (host_key && mentions_target)
        || (host_key && !mentions_via);
    if jump && !target {
        return "跳板机出错";
    }
    if target && !jump {
        return "目标机出错";
    }
    if jump && target {
        if contains_bounded(stderr, &target_denied)
            && !contains_bounded(stderr, &jump_denied)
            && !contains_bounded(stderr, &jump_connect)
            && !contains_bounded(stderr, &jump_dns)
        {
            return "目标机出错";
        }
        return "跳板机出错";
    }
    if unreachable_only(stderr) {
        return "中转命令或目标机不可达";
    }
    "未知"
}

fn unreachable_only(stderr: &str) -> bool {
    let mut saw = false;
    for line in stderr.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let marker = line.contains("Connection closed by UNKNOWN port 65535")
            || line.contains("kex_exchange_identification");
        if !marker {
            return false;
        }
        saw = true;
    }
    saw
}

fn contains_bounded(haystack: &str, phrase: &str) -> bool {
    if phrase.is_empty() {
        return false;
    }
    haystack.match_indices(phrase).any(|(index, _)| {
        let after = index + phrase.len();
        match haystack[after..].chars().next() {
            None => true,
            Some(ch) => !is_name_char(ch),
        }
    })
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '.'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::parse_topology;

    fn ends() -> RelayEnds {
        RelayEnds {
            via_host: "jump.example.com".to_string(),
            via_user: "me".to_string(),
            target_host: "192.0.2.30".to_string(),
            target_user: "ops".to_string(),
        }
    }

    #[test]
    fn each_blocked_class_is_rejected_by_parse() {
        let samples = [
            "nc %h %p < x",
            "nc (%h) %p",
            "nc %h %p)",
            "nc %h %p*",
            "nc %h %p?",
            "nc %h%p!",
            "nc ~/%h %p",
            "nc %h %p #",
            "nc %h %p\r",
            "nc %h %p\u{1}",
            "nc %h %p\u{7f}",
            "nc %h %p %",
            "nc %%h %p",
        ];
        for command in samples {
            let json = format!(
                r#"{{"regions":[{{"name":"r","devices":[{{"name":"j","host":"jump.example.com","user":"me"}},{{"name":"t","host":"192.0.2.9","user":"ops","connections":[{{"name":"c","via":"j","command":{}}}]}}]}}]}}"#,
                serde_json::to_string(command).unwrap()
            );
            assert!(
                parse_topology(&json).is_err(),
                "parse should reject {command:?}"
            );
            assert!(
                expand_relay_template(command, "192.0.2.9", 22).is_err(),
                "expand should reject {command:?}"
            );
        }
    }

    #[test]
    fn allowed_punctuation_expands() {
        let expanded = expand_relay_template("ab_c.d/e:f=g@h+i-j %h %p", "app.example.com", 22)
            .expect("allowlist");
        assert_eq!(expanded, "ab_c.d/e:f=g@h+i-j app.example.com 22");
    }

    #[test]
    fn doubled_percent_is_escaped_inside_proxy_command() {
        let json = r#"{"regions":[{"name":"r","devices":[
            {"name":"j","host":"jump.example.com","user":"me"},
            {"name":"t","host":"192.0.2.30","port":2200,"user":"ops","connections":[
                {"name":"c","via":"j","command":"nc %h %p %%"}]}]}]}"#;
        let topology = parse_topology(json).unwrap();
        let args = ssh_args_for_connection(&topology, "t", Some("c")).unwrap();
        let proxy = args
            .iter()
            .find(|arg| arg.starts_with("ProxyCommand="))
            .expect("ProxyCommand");
        assert!(proxy.ends_with("nc 192.0.2.30 2200 %%"), "{proxy}");
        assert_eq!(proxy.matches('%').count(), 2, "{proxy}");
        assert!(!args.iter().any(|arg| arg == "-J"));
    }

    #[test]
    fn via_that_jumps_or_relays_is_rejected() {
        let jumped = r#"{"regions":[{"name":"r","devices":[
            {"name":"j","host":"jump.example.com","user":"me","jump":"t"},
            {"name":"t","host":"192.0.2.9","user":"ops","connections":[
                {"name":"c","via":"j","command":"nc %h %p"}]}]}]}"#;
        let err = parse_topology(jumped).unwrap_err();
        assert!(err.contains("中转跳板机不能再有 jump"), "{err}");

        let nested = r#"{"regions":[{"name":"r","devices":[
            {"name":"a","host":"jump.example.com","user":"me"},
            {"name":"b","host":"192.0.2.8","user":"me","connections":[
                {"name":"c","via":"a","command":"nc %h %p"}]},
            {"name":"t","host":"192.0.2.9","user":"ops","connections":[
                {"name":"c","via":"b","command":"nc %h %p"}]}]}]}"#;
        let err = parse_topology(nested).unwrap_err();
        assert!(err.contains("中转跳板机不能再有 jump"), "{err}");
    }

    #[test]
    fn program_path_is_quoted_only_when_it_has_a_space() {
        assert_eq!(
            proxy_program(Path::new(r"C:\Windows\System32\OpenSSH\ssh.exe")).unwrap(),
            r"C:\Windows\System32\OpenSSH\ssh.exe"
        );
        assert_eq!(
            proxy_program(Path::new("/Program Files/ssh")).unwrap(),
            "\"/Program Files/ssh\""
        );
        assert!(proxy_program(Path::new("/usr/bin/ssh\"x")).is_err());
        assert!(proxy_program(Path::new("/usr/bin/ssh%x")).is_err());
        assert!(proxy_program(Path::new("/usr/bin/ssh;x")).is_err());
    }

    #[test]
    fn failure_line_names_jump_target_or_relay() {
        let ends = ends();
        assert_eq!(
            relay_failure_line(
                "ssh: connect to host jump.example.com port 2222: Connection refused\n",
                255,
                &ends
            ),
            Some("跳板机出错")
        );
        assert_eq!(
            relay_failure_line(
                "ssh: Could not resolve hostname jump.example.com: failure\n",
                255,
                &ends
            ),
            Some("跳板机出错")
        );
        assert_eq!(
            relay_failure_line(
                "me@jump.example.com: Permission denied (publickey).\n",
                255,
                &ends
            ),
            Some("跳板机出错")
        );
        assert_eq!(
            relay_failure_line(
                "ops@192.0.2.30: Permission denied (publickey).\n",
                255,
                &ends
            ),
            Some("目标机出错")
        );
        assert_eq!(
            relay_failure_line("Host key verification failed.\n", 255, &ends),
            Some("目标机出错")
        );
        assert_eq!(
            relay_failure_line(
                "Host key for jump.example.com\nHost key verification failed.\n",
                255,
                &ends
            ),
            Some("跳板机出错")
        );
        assert_eq!(
            relay_failure_line(
                "Connection closed by UNKNOWN port 65535\nkex_exchange_identification: Connection closed by remote host\n",
                255,
                &ends
            ),
            Some("中转命令或目标机不可达")
        );
        assert_eq!(
            relay_failure_line("something else\n", 255, &ends),
            Some("未知")
        );
        assert_eq!(
            relay_failure_line("ops@192.0.2.30: Permission denied\n", 0, &ends),
            None
        );
        assert_eq!(
            failure_side("Connection closed by UNKNOWN port 65535\n", 255),
            "unknown"
        );
        assert_eq!(
            failure_side("connect to host jump.example.com\n", 1),
            "unknown"
        );
    }

    #[test]
    fn example_keeps_proxyjump_and_adds_a_relay() {
        let text = include_str!("../../../topology.example.json");
        let topology = parse_topology(text).expect("example");
        let default = ssh_args_for_connection(&topology, "内网服务器", None).unwrap();
        assert!(default.iter().any(|arg| arg == "-J"), "{default:?}");
        let relay = ssh_args_for_connection(&topology, "内网服务器", Some("经跳板命令")).unwrap();
        assert!(!relay.iter().any(|arg| arg == "-J"), "{relay:?}");
        assert!(relay
            .iter()
            .any(|arg| { arg.ends_with(" -- jump.example.com tailscale nc app.example.com 22") }));
    }

    #[test]
    fn relay_plan_skips_the_jump_config_file() {
        let dir = std::env::temp_dir().join(format!("mt-relay-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("topology.json"),
            include_str!("../../tests/fixtures/topology.relay.json"),
        )
        .unwrap();
        let default = super::super::args::plan_device_session(&dir, "二级内网").unwrap();
        assert!(default.args.iter().any(|arg| arg == "-J"));
        assert!(!default
            .args
            .iter()
            .any(|arg| arg.starts_with("ProxyCommand=")));
        assert!(default.temp_file.is_some());
        drop(default);
        let relay =
            super::super::args::plan_device_session_with(&dir, "二级内网", Some("经跳板命令"))
                .unwrap();
        assert!(!relay.args.iter().any(|arg| arg == "-J" || arg == "-F"));
        assert!(relay
            .args
            .iter()
            .any(|arg| arg.starts_with("ProxyCommand=")));
        assert!(relay.temp_file.is_none());
        assert!(relay.relay.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

use crate::hosts::exec::RunOutput;
use crate::hosts::parse::{parse_status, TailnetStatus, TailscalePeer};
use crate::hosts::store::ManualHost;

pub const MSG_NOT_INSTALLED: &str = "未检测到 Tailscale，仅显示手动添加的服务器";
pub const MSG_NEEDS_LOGIN: &str = "Tailscale 未登录，请先登录";
pub const MSG_NOT_RUNNING: &str = "Tailscale 未运行";
pub const MSG_TIMEOUT: &str = "读取 Tailscale 状态超时";
pub const MSG_PARSE: &str = "Tailscale 状态解析失败";
pub const MSG_EXEC: &str = "读取 Tailscale 状态失败";
pub const MSG_UNKNOWN_STATE: &str = "无法识别的 Tailscale 状态";

/// One row in the server list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostEntry {
    pub id: String,
    pub name: String,
    pub host: String,
    pub ip: Option<String>,
    pub online: Option<bool>,
    pub os: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    /// `tailscale`, `manual`, or `tailscale+manual`.
    pub source: String,
    pub connect_target: String,
}

/// Payload returned by `list_hosts`. Manual servers are always present.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListHostsResponse {
    pub status: String,
    pub hosts: Vec<HostEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_name: Option<String>,
    /// Set when the JSON config was reset or comes from a newer version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_notice: Option<String>,
}

pub fn compose_list(output: &RunOutput, manuals: &[ManualHost]) -> ListHostsResponse {
    match output {
        RunOutput::NotInstalled => response(
            "not_installed",
            manuals_only(manuals),
            Some(MSG_NOT_INSTALLED.to_string()),
            None,
        ),
        RunOutput::TimedOut => response(
            "error",
            manuals_only(manuals),
            Some(MSG_TIMEOUT.to_string()),
            None,
        ),
        RunOutput::Failed => response(
            "error",
            manuals_only(manuals),
            Some(MSG_EXEC.to_string()),
            None,
        ),
        RunOutput::Finished { code, stdout } => match parse_status(stdout) {
            Ok(status) => from_status(status, manuals),
            Err(_) if *code == Some(0) => response(
                "error",
                manuals_only(manuals),
                Some(MSG_PARSE.to_string()),
                None,
            ),
            Err(_) => response(
                "not_running",
                manuals_only(manuals),
                Some(MSG_NOT_RUNNING.to_string()),
                None,
            ),
        },
    }
}

fn from_status(status: TailnetStatus, manuals: &[ManualHost]) -> ListHostsResponse {
    let hosts = merge_hosts(&status.peers, manuals);
    let self_name = status.self_name;
    match status.backend_state.as_str() {
        "Running" => response("ok", hosts, None, self_name),
        "NeedsLogin" | "NoState" => response(
            "needs_login",
            hosts,
            Some(MSG_NEEDS_LOGIN.to_string()),
            self_name,
        ),
        "Stopped" => response(
            "not_running",
            hosts,
            Some(MSG_NOT_RUNNING.to_string()),
            self_name,
        ),
        _ => response(
            "error",
            hosts,
            Some(MSG_UNKNOWN_STATE.to_string()),
            self_name,
        ),
    }
}

fn response(
    status: &str,
    hosts: Vec<HostEntry>,
    message: Option<String>,
    self_name: Option<String>,
) -> ListHostsResponse {
    ListHostsResponse {
        status: status.to_string(),
        hosts,
        message,
        self_name,
        config_notice: None,
    }
}

/// Merge tailnet peers with manual servers.
///
/// A manual host matches a peer when its host, compared case-insensitively,
/// equals that peer's MagicDNS name, IPv4, or HostName. The manual name, user,
/// and port win; online state and IP come from the tailnet. Rows are ordered
/// online first, then by name.
pub fn merge_hosts(peers: &[TailscalePeer], manuals: &[ManualHost]) -> Vec<HostEntry> {
    let mut peer_used = vec![false; peers.len()];
    let mut manual_used = vec![false; manuals.len()];
    let mut entries = Vec::new();

    for (manual_index, manual) in manuals.iter().enumerate() {
        let key = normalize(&manual.host);
        if key.is_empty() {
            continue;
        }
        if let Some(peer_index) = peers.iter().enumerate().position(|(peer_index, peer)| {
            !peer_used[peer_index] && peer_keys(peer).iter().any(|candidate| candidate == &key)
        }) {
            peer_used[peer_index] = true;
            manual_used[manual_index] = true;
            entries.push(merged_entry(&peers[peer_index], manual));
        }
    }

    for (peer_index, peer) in peers.iter().enumerate() {
        if !peer_used[peer_index] {
            entries.push(tailscale_entry(peer));
        }
    }
    for (manual_index, manual) in manuals.iter().enumerate() {
        if !manual_used[manual_index] {
            entries.push(manual_entry(manual));
        }
    }

    entries.sort_by(|left, right| {
        let left_online = left.online.unwrap_or(false);
        let right_online = right.online.unwrap_or(false);
        right_online
            .cmp(&left_online)
            .then_with(|| normalize(&left.name).cmp(&normalize(&right.name)))
            .then_with(|| left.id.cmp(&right.id))
    });
    entries
}

fn peer_keys(peer: &TailscalePeer) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(dns) = &peer.dns_name {
        keys.push(normalize(dns));
    }
    if let Some(ipv4) = &peer.ipv4 {
        keys.push(normalize(ipv4));
    }
    if !peer.host_name.is_empty() {
        keys.push(normalize(&peer.host_name));
    }
    keys
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

fn tailscale_entry(peer: &TailscalePeer) -> HostEntry {
    let name = if !peer.host_name.is_empty() {
        peer.host_name.clone()
    } else {
        peer.connect_target.clone()
    };
    HostEntry {
        id: format!("ts:{}", peer.node_id),
        name,
        host: peer.connect_target.clone(),
        ip: peer.ip.clone(),
        online: Some(peer.online),
        os: peer.os.clone(),
        user: None,
        port: None,
        source: "tailscale".to_string(),
        connect_target: peer.connect_target.clone(),
    }
}

fn manual_entry(manual: &ManualHost) -> HostEntry {
    let ip = if is_ip(&manual.host) {
        Some(manual.host.clone())
    } else {
        None
    };
    HostEntry {
        id: manual.id.clone(),
        name: manual.name.clone(),
        host: manual.host.clone(),
        ip,
        online: None,
        os: None,
        user: manual.user.clone(),
        port: Some(manual.port),
        source: "manual".to_string(),
        connect_target: manual.host.clone(),
    }
}

fn merged_entry(peer: &TailscalePeer, manual: &ManualHost) -> HostEntry {
    HostEntry {
        id: manual.id.clone(),
        name: manual.name.clone(),
        host: manual.host.clone(),
        ip: peer.ip.clone(),
        online: Some(peer.online),
        os: peer.os.clone(),
        user: manual.user.clone(),
        port: Some(manual.port),
        source: "tailscale+manual".to_string(),
        connect_target: peer.connect_target.clone(),
    }
}

fn manuals_only(manuals: &[ManualHost]) -> Vec<HostEntry> {
    let mut entries: Vec<HostEntry> = manuals.iter().map(manual_entry).collect();
    entries.sort_by(|left, right| normalize(&left.name).cmp(&normalize(&right.name)));
    entries
}

fn is_ip(value: &str) -> bool {
    value.parse::<std::net::Ipv4Addr>().is_ok() || value.parse::<std::net::Ipv6Addr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::parse::parse_status;
    use crate::hosts::store::ManualHost;

    fn manual(id: &str, name: &str, host: &str, port: u16) -> ManualHost {
        ManualHost {
            id: id.to_string(),
            name: name.to_string(),
            host: host.to_string(),
            user: Some("user".to_string()),
            port,
        }
    }

    fn finished(json: &str, code: i32) -> RunOutput {
        RunOutput::Finished {
            code: Some(code),
            stdout: json.to_string(),
        }
    }

    #[test]
    fn merges_when_manual_host_matches_dns_ipv4_or_hostname() {
        let status = parse_status(include_str!("../../fixtures/running.json")).unwrap();
        let manuals = vec![
            manual("manual-1", "Lab A", "HOST-A.example-tailnet.ts.net", 2201),
            manual("manual-2", "Lab B", "100.64.0.3", 2202),
            manual("manual-3", "Lab C", "Host-C", 2203),
            manual("manual-4", "Elsewhere", "other.example", 22),
        ];
        let entries = merge_hosts(&status.peers, &manuals);

        let lab_a = entries.iter().find(|entry| entry.id == "manual-1").unwrap();
        assert_eq!(lab_a.source, "tailscale+manual");
        assert_eq!(lab_a.name, "Lab A");
        assert_eq!(lab_a.user.as_deref(), Some("user"));
        assert_eq!(lab_a.port, Some(2201));
        assert_eq!(lab_a.online, Some(true));
        assert_eq!(lab_a.ip.as_deref(), Some("100.64.0.2"));
        assert_eq!(lab_a.connect_target, "host-a.example-tailnet.ts.net");

        let lab_b = entries.iter().find(|entry| entry.id == "manual-2").unwrap();
        assert_eq!(lab_b.source, "tailscale+manual");
        assert_eq!(lab_b.online, Some(false));
        assert_eq!(lab_b.ip.as_deref(), Some("100.64.0.3"));
        assert_eq!(lab_b.name, "Lab B");

        let lab_c = entries.iter().find(|entry| entry.id == "manual-3").unwrap();
        assert_eq!(lab_c.source, "tailscale+manual");
        assert_eq!(lab_c.connect_target, "100.64.0.4");
        assert_eq!(lab_c.port, Some(2203));

        let elsewhere = entries.iter().find(|entry| entry.id == "manual-4").unwrap();
        assert_eq!(elsewhere.source, "manual");
        assert_eq!(elsewhere.connect_target, "other.example");
        assert_eq!(elsewhere.online, None);

        let host_d = entries.iter().find(|entry| entry.name == "host-d").unwrap();
        assert_eq!(host_d.source, "tailscale");
        assert!(entries.iter().all(|entry| entry.name != "self-host"));
        assert!(
            entries
                .iter()
                .filter(|entry| entry.source == "tailscale+manual")
                .count()
                == 3
        );
    }

    #[test]
    fn does_not_merge_ipv6_or_partial_names() {
        let status = parse_status(include_str!("../../fixtures/running.json")).unwrap();
        let manuals = vec![
            manual("manual-1", "v6", "fd7a:115c:a1e0::2", 22),
            manual("manual-2", "short", "host-a.example", 22),
        ];
        let entries = merge_hosts(&status.peers, &manuals);
        assert!(entries
            .iter()
            .all(|entry| entry.source != "tailscale+manual"));
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.source == "manual")
                .count(),
            2
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.source == "tailscale")
                .count(),
            4
        );
    }

    #[test]
    fn sorts_online_hosts_before_the_rest_then_by_name() {
        let status = parse_status(include_str!("../../fixtures/running.json")).unwrap();
        let manuals = vec![manual("manual-9", "aaa", "other.example", 22)];
        let entries = merge_hosts(&status.peers, &manuals);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        // host-a and host-c are online. host-b, host-d, and the manual row are not.
        assert_eq!(names, ["host-a", "host-c", "aaa", "host-b", "host-d"]);
    }

    #[test]
    fn not_installed_returns_manuals_and_message() {
        let manuals = vec![manual("manual-1", "Lab", "host-a", 22)];
        let listed = compose_list(&RunOutput::NotInstalled, &manuals);
        assert_eq!(listed.status, "not_installed");
        assert_eq!(listed.message.as_deref(), Some(MSG_NOT_INSTALLED));
        assert_eq!(listed.self_name, None);
        assert_eq!(listed.hosts.len(), 1);
        assert_eq!(listed.hosts[0].source, "manual");
        assert_eq!(listed.hosts[0].name, "Lab");
    }

    #[test]
    fn needs_login_and_no_state_keep_manuals() {
        let manuals = vec![manual("manual-1", "Lab", "host-a", 22)];
        let needs_login = compose_list(
            &finished(include_str!("../../fixtures/needs_login.json"), 1),
            &manuals,
        );
        assert_eq!(needs_login.status, "needs_login");
        assert_eq!(needs_login.message.as_deref(), Some(MSG_NEEDS_LOGIN));
        assert_eq!(needs_login.hosts.len(), 1);
        assert_eq!(needs_login.hosts[0].source, "manual");

        let no_state = compose_list(
            &finished(r#"{"BackendState":"NoState","Peer":null}"#, 1),
            &manuals,
        );
        assert_eq!(no_state.status, "needs_login");
        assert_eq!(no_state.hosts.len(), 1);
    }

    #[test]
    fn stopped_backend_or_nonzero_exit_is_not_running() {
        let manuals = vec![manual("manual-1", "Lab", "host-a", 22)];
        let stopped = compose_list(
            &finished(include_str!("../../fixtures/stopped.json"), 1),
            &manuals,
        );
        assert_eq!(stopped.status, "not_running");
        assert_eq!(stopped.message.as_deref(), Some(MSG_NOT_RUNNING));
        assert_eq!(stopped.self_name.as_deref(), Some("self-host"));
        assert_eq!(stopped.hosts.len(), 1);
        assert_eq!(stopped.hosts[0].source, "manual");

        let failed = compose_list(&finished("tailscale failed", 1), &manuals);
        assert_eq!(failed.status, "not_running");
        assert_eq!(failed.hosts.len(), 1);
        assert_eq!(failed.hosts[0].id, "manual-1");
    }

    #[test]
    fn timeout_and_bad_json_are_errors_and_keep_manuals() {
        let manuals = vec![manual("manual-1", "Lab", "100.64.0.9", 22)];
        let timed_out = compose_list(&RunOutput::TimedOut, &manuals);
        assert_eq!(timed_out.status, "error");
        assert_eq!(timed_out.message.as_deref(), Some(MSG_TIMEOUT));
        assert_eq!(timed_out.hosts.len(), 1);
        assert_eq!(timed_out.hosts[0].ip.as_deref(), Some("100.64.0.9"));

        let failed = compose_list(&RunOutput::Failed, &manuals);
        assert_eq!(failed.status, "error");
        assert_eq!(failed.message.as_deref(), Some(MSG_EXEC));
        assert_eq!(failed.hosts[0].source, "manual");

        let malformed = compose_list(
            &finished(include_str!("../../fixtures/malformed.json"), 0),
            &manuals,
        );
        assert_eq!(malformed.status, "error");
        assert_eq!(malformed.message.as_deref(), Some(MSG_PARSE));
        assert_eq!(malformed.hosts.len(), 1);
    }

    #[test]
    fn running_status_is_ok_and_serializes_camel_case() {
        let listed = compose_list(
            &finished(include_str!("../../fixtures/running.json"), 0),
            &[],
        );
        assert_eq!(listed.status, "ok");
        assert_eq!(listed.message, None);
        assert_eq!(listed.self_name.as_deref(), Some("self-host"));
        assert_eq!(listed.hosts.len(), 4);
        assert!(listed.hosts.iter().all(|entry| entry.source == "tailscale"));

        let json = serde_json::to_value(&listed).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["selfName"], "self-host");
        assert!(json.get("message").is_none());
        assert_eq!(
            json["hosts"][0]["connectTarget"],
            "host-a.example-tailnet.ts.net"
        );
        assert_eq!(json["hosts"][0]["source"], "tailscale");
    }

    #[test]
    fn unknown_backend_state_is_error() {
        let listed = compose_list(
            &finished(r#"{"BackendState":"Starting","Peer":{}}"#, 0),
            &[],
        );
        assert_eq!(listed.status, "error");
        assert_eq!(listed.message.as_deref(), Some(MSG_UNKNOWN_STATE));
    }

    #[test]
    fn config_notice_serializes_only_when_present() {
        let mut listed = compose_list(&RunOutput::NotInstalled, &[]);
        let omitted = serde_json::to_value(&listed).unwrap();
        assert!(omitted.get("configNotice").is_none());

        listed.config_notice = Some("配置文件损坏，已备份并重置".to_string());
        let json = serde_json::to_value(&listed).unwrap();
        assert_eq!(json["configNotice"], "配置文件损坏，已备份并重置");
    }
}

/// Cases from issue #13. The macOS app binary prints a GUI error and exits 0
/// when `TERM` is not exactly `dumb`; these tests pin that behavior.
#[cfg(test)]
mod triage_tailscale_parse {
    use super::*;
    use crate::hosts::exec::{locate_tailscale_cli, run_command};
    use crate::hosts::parse::parse_status;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const REAL: &str = include_str!("../../fixtures/tailscale/status_1_104_mac.json");
    const NULL_IPS: &str = include_str!("../../fixtures/tailscale/status_1_104_null_ips.json");

    #[test]
    fn check_real_shape_1_104_parses() {
        let status = parse_status(REAL).expect("1.104 shape");
        assert_eq!(status.backend_state, "Running");
        assert_eq!(status.peers.len(), 4);
    }

    #[test]
    fn p1_null_tailscale_ips_is_tolerated() {
        let status = parse_status(NULL_IPS).expect("null TailscaleIPs must be tolerated");
        assert_eq!(status.peers.len(), 4);
        assert_eq!(status.self_name.as_deref(), Some("example-self"));
    }

    #[test]
    fn p2_empty_stdout_reports_specific_reason() {
        let out = RunOutput::Finished {
            code: Some(0),
            stdout: String::new(),
        };
        let msg = compose_list(&out, &[]).message.unwrap_or_default();
        assert_ne!(msg, MSG_PARSE, "需带具体原因，如『输出为空』");
    }

    #[test]
    fn p3_bad_json_message_includes_position() {
        let out = RunOutput::Finished {
            code: Some(0),
            stdout: "{\"BackendState\": ".into(),
        };
        let msg = compose_list(&out, &[]).message.unwrap_or_default();
        assert!(msg.contains("line") || msg.contains("行"), "msg={msg}");
    }

    #[cfg(unix)]
    #[test]
    fn p4_nonzero_exit_carries_stderr_summary() {
        let out = run_command(
            Path::new("/bin/sh"),
            &[
                "-c",
                "echo 'failed to connect to local tailscaled' >&2; exit 1",
            ],
            Duration::from_secs(2),
        );
        let msg = compose_list(&out, &[]).message.unwrap_or_default();
        assert!(msg.contains("failed to connect"), "msg={msg}");
    }

    #[test]
    fn p5_gui_path_finds_usr_local_bin_wrapper() {
        let want = PathBuf::from("/usr/local/bin/tailscale");
        let found = locate_tailscale_cli(None, |path| path == want.as_path(), None);
        assert_eq!(found, Some(want));
    }

    /// Fake CLI: JSON only when `TERM` is exactly `dumb`. Any other value,
    /// including empty, prints the GUI error and exits 0.
    #[cfg(unix)]
    fn fake_tailscale() -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("fake-ts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = dir.join("status.json");
        std::fs::write(&json, REAL).unwrap();
        let bin = dir.join("tailscale");
        let script = format!(
            "#!/bin/sh\nif [ \"$TERM\" = dumb ]; then cat '{path}'; else printf '%s\\n' 'The Tailscale GUI failed to start: placeholder reason'; exit 0; fi\n",
            path = json.display(),
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    /// Parent `TERM` is often already `dumb` in CI, which a child would inherit.
    /// Force a different value around the spawn so only `Command`'s own
    /// `TERM=dumb` can make the fake CLI emit JSON.
    #[cfg(unix)]
    struct ParentTermGuard(Option<std::ffi::OsString>);

    #[cfg(unix)]
    impl ParentTermGuard {
        fn force_not_dumb() -> Self {
            let previous = std::env::var_os("TERM");
            // `set_var` is safe before Rust 1.87 and unsafe afterwards.
            // The block compiles on both.
            #[allow(unused_unsafe)]
            unsafe {
                std::env::set_var("TERM", "not-dumb");
            }
            Self(previous)
        }
    }

    #[cfg(unix)]
    impl Drop for ParentTermGuard {
        fn drop(&mut self) {
            #[allow(unused_unsafe)]
            unsafe {
                match self.0.take() {
                    Some(value) => std::env::set_var("TERM", value),
                    None => std::env::remove_var("TERM"),
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn p6_child_env_sets_term_so_app_binary_runs_cli_mode() {
        let _guard = ParentTermGuard::force_not_dumb();
        let out = run_command(
            &fake_tailscale(),
            crate::hosts::exec::TAILSCALE_STATUS_ARGS,
            Duration::from_secs(3),
        );
        let listed = compose_list(&out, &[]);
        assert_eq!(
            listed.status, "ok",
            "子进程未带 TERM=dumb，拿到 GUI 报错：{:?}",
            listed.message
        );
        assert_eq!(listed.hosts.len(), 4);
    }

    #[test]
    fn p6b_gui_error_text_is_reported_specifically() {
        let out = RunOutput::Finished {
            code: Some(0),
            stdout: "The Tailscale GUI failed to start: placeholder reason\n".into(),
        };
        let msg = compose_list(&out, &[]).message.unwrap_or_default();
        assert!(
            msg.contains("GUI") || msg.contains("不是 JSON"),
            "msg={msg}"
        );
    }
}

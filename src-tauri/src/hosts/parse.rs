use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use serde::Deserialize;

/// Failure to parse `tailscale status --json`. Invalid input returns this error
/// instead of panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parsed tailnet snapshot. `Self` is exposed only as `self_name` and is never
/// included in `peers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailnetStatus {
    pub backend_state: String,
    pub self_name: Option<String>,
    pub peers: Vec<TailscalePeer>,
}

/// One peer machine from the `Peer` map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscalePeer {
    pub node_id: String,
    pub host_name: String,
    pub dns_name: Option<String>,
    /// Address to display: the first IPv4, or the first IPv6 when no IPv4 exists.
    pub ip: Option<String>,
    /// First IPv4, when the peer has one. Used as a merge key.
    pub ipv4: Option<String>,
    pub online: bool,
    pub os: Option<String>,
    /// MagicDNS name when present, otherwise the display IP.
    pub connect_target: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawStatus {
    #[serde(rename = "BackendState", default)]
    backend_state: String,
    #[serde(rename = "Self", default)]
    self_node: Option<RawNode>,
    #[serde(rename = "Peer", default)]
    peers: Option<BTreeMap<String, RawNode>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawNode {
    #[serde(rename = "HostName", default)]
    host_name: String,
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "OS", default)]
    os: String,
    #[serde(rename = "Online", default)]
    online: bool,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<String>,
}

/// Parse `tailscale status --json` text. Unknown fields are ignored.
pub fn parse_status(json: &str) -> Result<TailnetStatus, ParseError> {
    let raw: RawStatus = serde_json::from_str(json).map_err(|err| ParseError {
        message: format!("invalid tailscale status json: {err}"),
    })?;

    let mut peers = Vec::new();
    if let Some(map) = raw.peers {
        for (node_id, node) in map {
            if let Some(peer) = peer_from_node(node_id, node) {
                peers.push(peer);
            }
        }
    }

    Ok(TailnetStatus {
        backend_state: raw.backend_state,
        self_name: raw.self_node.as_ref().and_then(node_display_name),
        peers,
    })
}

fn peer_from_node(node_id: String, node: RawNode) -> Option<TailscalePeer> {
    let host_name = node.host_name.trim().to_string();
    let dns_name = strip_dns(&node.dns_name);
    let (ip, ipv4) = choose_ip(&node.tailscale_ips);
    let connect_target = dns_name.clone().or_else(|| ip.clone()).or_else(|| {
        if host_name.is_empty() {
            None
        } else {
            Some(host_name.clone())
        }
    })?;
    let os = {
        let trimmed = node.os.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    Some(TailscalePeer {
        node_id,
        host_name,
        dns_name,
        ip,
        ipv4,
        online: node.online,
        os,
        connect_target,
    })
}

fn node_display_name(node: &RawNode) -> Option<String> {
    let host = node.host_name.trim();
    if !host.is_empty() {
        return Some(host.to_string());
    }
    strip_dns(&node.dns_name)
}

fn strip_dns(name: &str) -> Option<String> {
    let trimmed = name.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// First IPv4 in the list, otherwise the first IPv6. Returns `(display, ipv4)`.
fn choose_ip(ips: &[String]) -> (Option<String>, Option<String>) {
    let mut ipv4 = None;
    let mut ipv6 = None;
    for ip in ips {
        let trimmed = ip.trim();
        if trimmed.is_empty() {
            continue;
        }
        if ipv4.is_none() && trimmed.parse::<Ipv4Addr>().is_ok() {
            ipv4 = Some(trimmed.to_string());
            break;
        }
        if ipv6.is_none() && trimmed.parse::<Ipv6Addr>().is_ok() {
            ipv6 = Some(trimmed.to_string());
        }
    }
    let display = ipv4.clone().or(ipv6);
    (display, ipv4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> TailnetStatus {
        parse_status(include_str!("../../fixtures/running.json")).expect("running fixture")
    }

    fn peer<'a>(status: &'a TailnetStatus, host_name: &str) -> &'a TailscalePeer {
        status
            .peers
            .iter()
            .find(|peer| peer.host_name == host_name)
            .unwrap_or_else(|| panic!("missing peer {host_name}"))
    }

    #[test]
    fn running_maps_fields_and_omits_self() {
        let status = running();
        assert_eq!(status.backend_state, "Running");
        assert_eq!(status.self_name.as_deref(), Some("self-host"));
        assert_eq!(status.peers.len(), 4);
        assert!(status
            .peers
            .iter()
            .all(|peer| peer.host_name != "self-host"));
        assert!(status
            .peers
            .iter()
            .all(|peer| peer.ip.as_deref() != Some("100.64.0.1")));
        assert!(status
            .peers
            .iter()
            .all(|peer| peer.connect_target != "self-host.example-tailnet.ts.net"));

        let host_a = peer(&status, "host-a");
        assert_eq!(host_a.os.as_deref(), Some("linux"));
        assert!(host_a.online);
        assert_eq!(
            host_a.dns_name.as_deref(),
            Some("host-a.example-tailnet.ts.net")
        );
        assert_eq!(host_a.connect_target, "host-a.example-tailnet.ts.net");

        let host_b = peer(&status, "host-b");
        assert!(!host_b.online);
        assert_eq!(host_b.os.as_deref(), Some("windows"));
        assert_eq!(host_b.ip.as_deref(), Some("100.64.0.3"));
    }

    #[test]
    fn ipv4_is_preferred_over_earlier_ipv6() {
        let status = running();
        let host_a = peer(&status, "host-a");
        assert_eq!(host_a.ip.as_deref(), Some("100.64.0.2"));
        assert_eq!(host_a.ipv4.as_deref(), Some("100.64.0.2"));
        assert_ne!(host_a.ip.as_deref(), Some("100.64.0.20"));
        assert_ne!(host_a.ip.as_deref(), Some("fd7a:115c:a1e0::2"));
    }

    #[test]
    fn trailing_dot_is_stripped_from_dns_name() {
        let status = running();
        let host_a = peer(&status, "host-a");
        assert_eq!(
            host_a.dns_name.as_deref(),
            Some("host-a.example-tailnet.ts.net")
        );
        assert!(!host_a.connect_target.ends_with('.'));
    }

    #[test]
    fn missing_dns_name_uses_ip_as_connect_target() {
        let status = running();
        let host_c = peer(&status, "host-c");
        assert_eq!(host_c.dns_name, None);
        assert_eq!(host_c.ip.as_deref(), Some("100.64.0.4"));
        assert_eq!(host_c.connect_target, "100.64.0.4");
        assert!(host_c.online);
        assert_eq!(host_c.os.as_deref(), Some("macOS"));
    }

    #[test]
    fn ipv6_only_peer_uses_ipv6() {
        let status = running();
        let host_d = peer(&status, "host-d");
        assert_eq!(host_d.ipv4, None);
        assert_eq!(host_d.ip.as_deref(), Some("fd7a:115c:a1e0::5"));
        assert_eq!(host_d.connect_target, "fd7a:115c:a1e0::5");
        assert!(!host_d.online);
    }

    #[test]
    fn needs_login_fixture_has_no_peers() {
        let status = parse_status(include_str!("../../fixtures/needs_login.json"))
            .expect("needs_login fixture");
        assert_eq!(status.backend_state, "NeedsLogin");
        assert_eq!(status.self_name, None);
        assert!(status.peers.is_empty());
    }

    #[test]
    fn stopped_fixture_keeps_self_name_out_of_peers() {
        let status =
            parse_status(include_str!("../../fixtures/stopped.json")).expect("stopped fixture");
        assert_eq!(status.backend_state, "Stopped");
        assert_eq!(status.self_name.as_deref(), Some("self-host"));
        assert!(status.peers.is_empty());
    }

    #[test]
    fn empty_peers_fixture_reports_self_name_only() {
        let status = parse_status(include_str!("../../fixtures/empty_peers.json"))
            .expect("empty_peers fixture");
        assert_eq!(status.backend_state, "Running");
        assert_eq!(status.self_name.as_deref(), Some("self-host"));
        assert!(status.peers.is_empty());
    }

    #[test]
    fn malformed_json_returns_err_without_panic() {
        let first = parse_status(include_str!("../../fixtures/malformed.json"));
        let second = parse_status("{");
        let third = parse_status("null");
        let fourth = parse_status("");
        assert!(first.is_err());
        assert!(second.is_err());
        assert!(third.is_err());
        assert!(fourth.is_err());
    }

    #[test]
    fn no_state_and_empty_object_parse() {
        let no_state = parse_status(r#"{"BackendState":"NoState","Peer":{}}"#).expect("NoState");
        assert_eq!(no_state.backend_state, "NoState");
        assert!(no_state.peers.is_empty());

        let empty = parse_status("{}").expect("empty object");
        assert_eq!(empty.backend_state, "");
        assert_eq!(empty.self_name, None);
        assert!(empty.peers.is_empty());
    }
}

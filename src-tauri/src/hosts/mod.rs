mod exec;
mod list;
mod parse;
mod store;

use std::path::Path;
use std::sync::Mutex;

use tauri::Manager;

pub use exec::tailscale_status;
pub use list::{compose_list, HostEntry, ListHostsResponse};
pub use parse::{parse_status, ParseError, TailnetStatus};
pub use store::{validate_manual_input, HostStore, ManualHost, MemoryHostStore, NewManualHost};

use exec::RunOutput;
use parse::TailscalePeer;

/// Whether a topology address matched a Tailscale peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressPresence {
    Online,
    Offline,
    Unknown,
}

/// Peer addresses from one `tailscale status --json` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleDirectory {
    readable: bool,
    peers: Vec<DirectoryPeer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryPeer {
    online: bool,
    keys: Vec<String>,
}

impl TailscaleDirectory {
    /// Run the same status command the host list uses.
    pub fn read() -> Self {
        Self::from_output(&tailscale_status())
    }

    pub fn from_output(output: &RunOutput) -> Self {
        let RunOutput::Finished { stdout, .. } = output else {
            return Self {
                readable: false,
                peers: Vec::new(),
            };
        };
        let status = match parse_status(stdout) {
            Ok(status) => status,
            Err(_) => {
                return Self {
                    readable: false,
                    peers: Vec::new(),
                };
            }
        };
        let raw: serde_json::Value =
            serde_json::from_str(stdout).unwrap_or(serde_json::Value::Null);
        let peers = status
            .peers
            .iter()
            .map(|peer| DirectoryPeer {
                online: peer.online,
                keys: peer_match_keys(&raw, peer),
            })
            .collect();
        Self {
            readable: true,
            peers,
        }
    }

    pub fn presence(&self, address: &str) -> AddressPresence {
        if !self.readable {
            return AddressPresence::Unknown;
        }
        let needle = normalize_match_key(address);
        if needle.is_empty() {
            return AddressPresence::Unknown;
        }
        for peer in &self.peers {
            if peer.keys.iter().any(|key| key == &needle) {
                return if peer.online {
                    AddressPresence::Online
                } else {
                    AddressPresence::Offline
                };
            }
        }
        AddressPresence::Unknown
    }
}

fn normalize_match_key(value: &str) -> String {
    value.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn peer_match_keys(raw: &serde_json::Value, peer: &TailscalePeer) -> Vec<String> {
    let mut keys = Vec::new();
    let mut push = |value: &str| {
        let key = normalize_match_key(value);
        if !key.is_empty() && !keys.contains(&key) {
            keys.push(key);
        }
    };
    if let Some(dns) = &peer.dns_name {
        push(dns);
    }
    if let Some(ip) = &peer.ip {
        push(ip);
    }
    if let Some(ipv4) = &peer.ipv4 {
        push(ipv4);
    }
    if let Some(node) = raw.get("Peer").and_then(|peers| peers.get(&peer.node_id)) {
        if let Some(dns) = node.get("DNSName").and_then(|value| value.as_str()) {
            push(dns);
        }
        if let Some(ips) = node.get("TailscaleIPs").and_then(|value| value.as_array()) {
            for ip in ips {
                if let Some(text) = ip.as_str() {
                    push(text);
                }
            }
        }
    }
    keys
}

use crate::config::JsonHostStore;
use list::compose_list as build_list;

/// Manual servers loaded from the local JSON config.
pub struct HostState {
    store: Mutex<JsonHostStore>,
}

impl HostState {
    pub fn open(dir: &Path) -> Self {
        Self {
            store: Mutex::new(JsonHostStore::open(dir)),
        }
    }

    fn with_store<T>(&self, f: impl FnOnce(&mut JsonHostStore) -> T) -> Result<T, String> {
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "内部状态不可用".to_string())?;
        Ok(f(&mut guard))
    }
}

/// Read Tailscale once and merge the result with manual servers.
///
/// Manual servers are cloned before the CLI runs, so the store lock is not
/// held across the await. The process itself runs on a blocking thread.
#[tauri::command]
pub async fn list_hosts(
    app: tauri::AppHandle,
    state: tauri::State<'_, HostState>,
) -> Result<ListHostsResponse, String> {
    crate::auth::require_unlocked(&app.state::<crate::auth::AuthState>())
        .map_err(|err| err.to_string())?;
    let (manuals, notice) = state.with_store(|store| {
        let notice = store.notice().map(ToOwned::to_owned);
        (store.list(), notice)
    })?;
    let output = tauri::async_runtime::spawn_blocking(tailscale_status)
        .await
        .map_err(|_| "读取 Tailscale 状态失败".to_string())?;
    let mut listed = build_list(&output, &manuals);
    listed.config_notice = notice;
    Ok(listed)
}

/// Validate, remember, and immediately save a manual server.
///
/// 碰磁盘或外部进程的 Tauri 命令一律 async，避免阻塞主线程。
/// 写盘放在 `spawn_blocking` 里；锁只在该闭包内持有，不跨 `.await`。
#[tauri::command]
pub async fn add_manual_host(
    app: tauri::AppHandle,
    name: String,
    host: String,
    user: Option<String>,
    port: Option<u32>,
) -> Result<ManualHost, String> {
    crate::auth::require_unlocked(&app.state::<crate::auth::AuthState>())
        .map_err(|err| err.to_string())?;
    let validated = validate_manual_input(&name, &host, user.as_deref(), port)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<HostState>();
        state.with_store(|store| store.add(validated))?
    })
    .await
    .map_err(|_| "保存服务器失败".to_string())?
}

/// Drop a manual server by the id returned when it was added.
///
/// 碰磁盘或外部进程的 Tauri 命令一律 async，避免阻塞主线程。
/// 写盘放在 `spawn_blocking` 里；锁只在该闭包内持有，不跨 `.await`。
#[tauri::command]
pub async fn remove_manual_host(app: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::auth::require_unlocked(&app.state::<crate::auth::AuthState>())
        .map_err(|err| err.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<HostState>();
        state.with_store(|store| store.remove(&id))?
    })
    .await
    .map_err(|_| "移除服务器失败".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(stdout: &str) -> RunOutput {
        RunOutput::Finished {
            code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
            program: "tailscale".to_string(),
        }
    }

    #[test]
    fn directory_matches_ip_and_magic_dns_without_the_local_node() {
        let directory =
            TailscaleDirectory::from_output(&finished(include_str!("../../fixtures/running.json")));
        assert_eq!(directory.presence("100.64.0.2"), AddressPresence::Online);
        assert_eq!(directory.presence("100.64.0.20"), AddressPresence::Online);
        assert_eq!(
            directory.presence("HOST-A.example-tailnet.ts.net."),
            AddressPresence::Online
        );
        assert_eq!(directory.presence("100.64.0.3"), AddressPresence::Offline);
        assert_eq!(directory.presence("198.51.100.9"), AddressPresence::Unknown);
        assert_eq!(directory.presence("100.64.0.1"), AddressPresence::Unknown);
    }

    #[test]
    fn unreadable_status_is_unknown() {
        let directory = TailscaleDirectory::from_output(&RunOutput::NotInstalled);
        assert_eq!(directory.presence("100.64.0.2"), AddressPresence::Unknown);
    }
}

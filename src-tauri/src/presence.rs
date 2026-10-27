//! Online state of topology devices, worked out on this computer only.
//! Nothing is installed or run on the devices.
//!
//! Each device, in order:
//! 1. A connected session tab: online, no probe.
//! 2. `tailscaleAddress` or `host` matches a Tailscale peer: the peer's
//!    online flag, with no traffic to the device.
//! 3. No `jump`: a TCP connect to the SSH port. The probe reads the server's
//!    version line, sends its own and closes, so sshd logs a plain
//!    `[preauth]` close. Connected is online; refused, timed out or
//!    unresolved is offline.
//! 4. Behind a jump host: this computer cannot reach it directly, and the
//!    probe never logs in to the jump host. Offline when the jump host is
//!    offline, otherwise unknown.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tauri::Manager;

use crate::hosts::{AddressPresence, TailscaleDirectory};
use crate::topology::{Device, Topology};

/// Connect and version-line wait, per address.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
/// A round never waits longer than this, even for a slow DNS lookup.
pub const ROUND_DEADLINE: Duration = Duration::from_secs(5);
/// Probes started in one round; the rest are unknown.
pub const MAX_PROBES: usize = 64;
/// Addresses tried per host.
const MAX_ADDRESSES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Presence {
    Online,
    Offline,
    Unknown,
}

/// Result of a TCP probe of the SSH port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Open,
    /// Refused, timed out, unreachable or not resolved.
    Closed,
}

/// What is known about one device in a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evidence {
    pub connected: bool,
    pub tailscale: AddressPresence,
    /// None when the device was not probed or the probe did not finish in time.
    pub probe: Option<Probe>,
    /// The jump host's result this round. None for a device without a jump.
    pub jump: Option<Presence>,
}

pub fn decide(evidence: Evidence) -> Presence {
    if evidence.connected {
        return Presence::Online;
    }
    match evidence.tailscale {
        AddressPresence::Online => return Presence::Online,
        AddressPresence::Offline => return Presence::Offline,
        AddressPresence::Unknown => {}
    }
    match evidence.probe {
        Some(Probe::Open) => return Presence::Online,
        Some(Probe::Closed) => return Presence::Offline,
        None => {}
    }
    if evidence.jump == Some(Presence::Offline) {
        return Presence::Offline;
    }
    Presence::Unknown
}

/// Tailscale's view of a device: its `tailscaleAddress` first, then `host`.
pub fn tailscale_presence(device: &Device, directory: &TailscaleDirectory) -> AddressPresence {
    if let Some(address) = &device.tailscale_address {
        let presence = directory.presence(address);
        if presence != AddressPresence::Unknown {
            return presence;
        }
    }
    directory.presence(&device.host)
}

/// TCP probe of `host:port`. The first address that accepts wins.
pub fn probe_ssh(host: &str, port: u16, timeout: Duration) -> Probe {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return Probe::Closed;
    };
    for address in addresses.take(MAX_ADDRESSES) {
        if let Ok(stream) = TcpStream::connect_timeout(&address, timeout) {
            exchange_versions(stream, timeout);
            return Probe::Open;
        }
    }
    Probe::Closed
}

/// Read the server's version line, send ours, close. Errors are ignored:
/// the port already accepted.
fn exchange_versions(mut stream: TcpStream, timeout: Duration) {
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let mut line = [0u8; 256];
    let mut read = 0;
    while read < line.len() {
        match stream.read(&mut line[read..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                read += n;
                if line[..read].contains(&b'\n') {
                    break;
                }
            }
        }
    }
    let ours = format!("SSH-2.0-my-terminal_{}\r\n", env!("CARGO_PKG_VERSION"));
    let _ = stream.write_all(ours.as_bytes());
    let _ = stream.shutdown(Shutdown::Both);
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DevicePresence {
    pub name: String,
    pub presence: Presence,
}

/// One round over the whole topology. `probe` is the TCP probe, replaced in tests.
pub fn presence_round<F>(
    topology: &Topology,
    directory: &TailscaleDirectory,
    connected: &[String],
    probe: F,
) -> Vec<DevicePresence>
where
    F: Fn(&str, u16) -> Probe + Send + Sync + Clone + 'static,
{
    let devices: Vec<&Device> = topology
        .regions
        .iter()
        .flat_map(|region| region.devices.iter())
        .collect();
    let mut known: HashMap<String, (bool, AddressPresence)> = HashMap::new();
    let mut targets: Vec<(String, String, u16)> = Vec::new();
    for device in &devices {
        let is_connected = connected.iter().any(|name| name == &device.name);
        let tailscale = if is_connected {
            AddressPresence::Unknown
        } else {
            tailscale_presence(device, directory)
        };
        known.insert(device.name.clone(), (is_connected, tailscale));
        if !is_connected
            && tailscale == AddressPresence::Unknown
            && device.jump.is_none()
            && targets.len() < MAX_PROBES
        {
            targets.push((device.name.clone(), device.host.clone(), device.port));
        }
    }

    let probes = run_probes(targets, probe);

    let by_name: HashMap<&str, &Device> = devices
        .iter()
        .map(|device| (device.name.as_str(), *device))
        .collect();
    let mut resolved: HashMap<String, Presence> = HashMap::new();
    devices
        .iter()
        .map(|device| DevicePresence {
            name: device.name.clone(),
            presence: resolve(&device.name, &by_name, &known, &probes, &mut resolved, 0),
        })
        .collect()
}

/// Probe every target on its own thread; whatever has not answered by the
/// deadline stays unknown.
fn run_probes<F>(targets: Vec<(String, String, u16)>, probe: F) -> HashMap<String, Probe>
where
    F: Fn(&str, u16) -> Probe + Send + Sync + Clone + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let expected = targets.len();
    for (name, host, port) in targets {
        let sender = sender.clone();
        let probe = probe.clone();
        std::thread::spawn(move || {
            let _ = sender.send((name, probe(&host, port)));
        });
    }
    drop(sender);
    let deadline = Instant::now() + ROUND_DEADLINE;
    let mut results = HashMap::new();
    while results.len() < expected {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok((name, result)) => {
                results.insert(name, result);
            }
            Err(_) => break,
        }
    }
    results
}

fn resolve(
    name: &str,
    by_name: &HashMap<&str, &Device>,
    known: &HashMap<String, (bool, AddressPresence)>,
    probes: &HashMap<String, Probe>,
    resolved: &mut HashMap<String, Presence>,
    depth: usize,
) -> Presence {
    if let Some(presence) = resolved.get(name) {
        return *presence;
    }
    let Some(device) = by_name.get(name) else {
        return Presence::Unknown;
    };
    let (connected, tailscale) = known
        .get(name)
        .copied()
        .unwrap_or((false, AddressPresence::Unknown));
    // The topology rejects jump cycles; the depth limit is only a guard.
    let jump = match &device.jump {
        Some(jump) if depth < 16 => {
            Some(resolve(jump, by_name, known, probes, resolved, depth + 1))
        }
        Some(_) => Some(Presence::Unknown),
        None => None,
    };
    let presence = decide(Evidence {
        connected,
        tailscale,
        probe: probes.get(name).copied(),
        jump,
    });
    resolved.insert(name.to_string(), presence);
    presence
}

/// Online state of every topology device. Takes device names of connected
/// tabs only; hosts and ports come from topology.json, never from the page.
#[tauri::command]
pub async fn probe_presence(
    app: tauri::AppHandle,
    connected: Vec<String>,
) -> Result<Vec<DevicePresence>, String> {
    {
        let auth = app.state::<crate::auth::AuthState>();
        crate::auth::require_access(&auth, auth.lock_enabled())
    }
    .map_err(|err| err.to_string())?;
    let dir =
        crate::config::resolve_app_config_dir(&app).map_err(|_| "无法定位配置目录".to_string())?;
    let connected: Vec<String> = connected.into_iter().take(MAX_PROBES).collect();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(topology) = crate::topology::read_topology_file(&dir) else {
            return Vec::new();
        };
        let directory = TailscaleDirectory::read();
        presence_round(&topology, &directory, &connected, |host, port| {
            probe_ssh(host, port, PROBE_TIMEOUT)
        })
    })
    .await
    .map_err(|_| "检测在线状态失败".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::RunOutput;
    use crate::topology::parse_topology;
    use std::net::TcpListener;

    const NOTHING: Evidence = Evidence {
        connected: false,
        tailscale: AddressPresence::Unknown,
        probe: None,
        jump: None,
    };

    #[test]
    fn decide_follows_the_documented_order() {
        let cases = [
            (NOTHING, Presence::Unknown),
            (
                Evidence {
                    connected: true,
                    tailscale: AddressPresence::Offline,
                    ..NOTHING
                },
                Presence::Online,
            ),
            (
                Evidence {
                    tailscale: AddressPresence::Online,
                    probe: Some(Probe::Closed),
                    ..NOTHING
                },
                Presence::Online,
            ),
            (
                Evidence {
                    tailscale: AddressPresence::Offline,
                    probe: Some(Probe::Open),
                    ..NOTHING
                },
                Presence::Offline,
            ),
            (
                Evidence {
                    probe: Some(Probe::Open),
                    ..NOTHING
                },
                Presence::Online,
            ),
            (
                Evidence {
                    probe: Some(Probe::Closed),
                    ..NOTHING
                },
                Presence::Offline,
            ),
            (
                Evidence {
                    jump: Some(Presence::Offline),
                    ..NOTHING
                },
                Presence::Offline,
            ),
            (
                Evidence {
                    jump: Some(Presence::Online),
                    ..NOTHING
                },
                Presence::Unknown,
            ),
            (
                Evidence {
                    jump: Some(Presence::Unknown),
                    ..NOTHING
                },
                Presence::Unknown,
            ),
        ];
        for (evidence, expected) in cases {
            assert_eq!(decide(evidence), expected, "{evidence:?}");
        }
    }

    #[test]
    fn an_open_port_is_open_and_a_closed_one_is_not() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"SSH-2.0-OpenSSH_9.6\r\n").unwrap();
            let mut seen = Vec::new();
            let _ = stream.read_to_end(&mut seen);
            String::from_utf8_lossy(&seen).into_owned()
        });
        assert_eq!(probe_ssh("127.0.0.1", port, PROBE_TIMEOUT), Probe::Open);
        let greeting = server.join().unwrap();
        assert!(greeting.starts_with("SSH-2.0-my-terminal_"), "{greeting}");
        assert!(greeting.ends_with("\r\n"), "{greeting}");

        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = closed.local_addr().unwrap().port();
        drop(closed);
        assert_eq!(
            probe_ssh("127.0.0.1", closed_port, PROBE_TIMEOUT),
            Probe::Closed
        );
    }

    #[test]
    fn an_unresolvable_host_is_closed() {
        assert_eq!(
            probe_ssh("no-such-host.invalid", 22, Duration::from_millis(300)),
            Probe::Closed
        );
    }

    /// 100.64.0.2 is online and 100.64.0.3 offline in the fixture.
    fn tailnet() -> TailscaleDirectory {
        TailscaleDirectory::from_output(&RunOutput::Finished {
            code: Some(0),
            stdout: include_str!("../fixtures/running.json").to_string(),
            stderr: String::new(),
            program: "tailscale".to_string(),
        })
    }

    const TOPOLOGY: &str = r#"{"regions":[
        {"name":"家","devices":[
            {"name":"nas","host":"192.0.2.5","user":"me","tailscaleAddress":"100.64.0.2"},
            {"name":"mac","host":"100.64.0.3","user":"me"},
            {"name":"router","host":"192.0.2.1","user":"admin"}
        ]},
        {"name":"云","devices":[
            {"name":"jump","host":"jump.example.com","user":"me"},
            {"name":"app","host":"192.0.2.30","user":"me","jump":"jump"},
            {"name":"deep","host":"192.0.2.31","user":"me","jump":"app"},
            {"name":"down","host":"down.example.com","user":"me"},
            {"name":"behind-down","host":"192.0.2.40","user":"me","jump":"down"},
            {"name":"busy","host":"busy.example.com","user":"me"}
        ]}
    ]}"#;

    #[test]
    fn a_round_uses_sessions_tailscale_and_probes_and_never_probes_through_a_jump() {
        let topology = parse_topology(TOPOLOGY).unwrap();
        let probed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = probed.clone();
        let result = presence_round(
            &topology,
            &tailnet(),
            &["busy".to_string()],
            move |host, port| {
                seen.lock().unwrap().push(format!("{host}:{port}"));
                match host {
                    "jump.example.com" | "192.0.2.1" => Probe::Open,
                    _ => Probe::Closed,
                }
            },
        );
        let get = |name: &str| {
            result
                .iter()
                .find(|item| item.name == name)
                .unwrap()
                .presence
        };
        assert_eq!(get("nas"), Presence::Online, "tailscaleAddress");
        assert_eq!(get("mac"), Presence::Offline, "host on the tailnet");
        assert_eq!(get("router"), Presence::Online, "probe");
        assert_eq!(get("jump"), Presence::Online, "probe");
        assert_eq!(get("app"), Presence::Unknown, "behind an online jump");
        assert_eq!(get("deep"), Presence::Unknown, "two jumps deep");
        assert_eq!(get("down"), Presence::Offline, "probe failed");
        assert_eq!(get("behind-down"), Presence::Offline, "jump host offline");
        assert_eq!(get("busy"), Presence::Online, "connected tab");
        assert_eq!(result.len(), 9);

        let mut probed = probed.lock().unwrap().clone();
        probed.sort();
        assert_eq!(
            probed,
            ["192.0.2.1:22", "down.example.com:22", "jump.example.com:22"],
            "Tailscale 已知、已连接、有 jump 的设备都不探测"
        );
    }

    #[test]
    fn a_probe_past_the_deadline_is_unknown() {
        let topology = parse_topology(
            r#"{"regions":[{"name":"r","devices":[{"name":"slow","host":"192.0.2.9","user":"me"}]}]}"#,
        )
        .unwrap();
        let empty = TailscaleDirectory::from_output(&RunOutput::NotInstalled);
        let start = Instant::now();
        let result = presence_round(&topology, &empty, &[], |_, _| {
            std::thread::sleep(ROUND_DEADLINE + Duration::from_secs(2));
            Probe::Open
        });
        assert!(start.elapsed() < ROUND_DEADLINE + Duration::from_secs(1));
        assert_eq!(result[0].presence, Presence::Unknown);
    }
}

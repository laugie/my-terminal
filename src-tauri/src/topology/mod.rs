mod args;
mod parse;
mod validate;

use std::path::Path;

use tauri::Manager;

pub use args::{plan_device_session, ssh_args_for, SessionPlan, TempSshConfig};
pub use parse::{parse_topology, Device, Region, Topology};

use crate::hosts::{AddressPresence, TailscaleDirectory};

pub const TOPOLOGY_FILE_NAME: &str = "topology.json";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologyView {
    pub config_path: String,
    /// `ok`, `missing`, `empty`, or `error`.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub regions: Vec<RegionView>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionView {
    pub name: String,
    pub devices: Vec<DeviceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub mobile: bool,
    /// `online`, `offline`, `unknown`, or `none`.
    pub presence: String,
}

/// Read `topology.json` from the app config directory.
///
/// Disk IO and the Tailscale status process run on a blocking thread.
/// The command does not hold a lock across `.await`.
#[tauri::command]
pub async fn load_topology(app: tauri::AppHandle) -> Result<TopologyView, String> {
    crate::auth::require_unlocked(&app.state::<crate::auth::AuthState>())
        .map_err(|err| err.to_string())?;
    let dir =
        crate::config::resolve_app_config_dir(&app).map_err(|_| "无法定位配置目录".to_string())?;
    let view = tauri::async_runtime::spawn_blocking(move || load_topology_from_dir(&dir))
        .await
        .map_err(|_| "读取拓扑配置失败".to_string())?;
    Ok(view)
}

pub fn load_topology_from_dir(dir: &Path) -> TopologyView {
    let config_path = dir.join(TOPOLOGY_FILE_NAME);
    let display = config_path.display().to_string();
    let text = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return TopologyView {
                config_path: display.clone(),
                state: "missing".to_string(),
                message: Some(format!(
                    "未找到拓扑配置。请创建 {display}，格式可参考仓库根目录的 topology.example.json。"
                )),
                regions: Vec::new(),
            };
        }
        Err(_) => {
            return TopologyView {
                config_path: display,
                state: "error".to_string(),
                message: Some("无法读取拓扑配置".to_string()),
                regions: Vec::new(),
            };
        }
    };
    if text.len() > 1_048_576 {
        return TopologyView {
            config_path: display,
            state: "error".to_string(),
            message: Some("拓扑配置过大".to_string()),
            regions: Vec::new(),
        };
    }
    let topology = match parse_topology(&text) {
        Ok(topology) => topology,
        Err(err) => {
            return TopologyView {
                config_path: display,
                state: "error".to_string(),
                message: Some(err),
                regions: Vec::new(),
            };
        }
    };
    let device_count: usize = topology
        .regions
        .iter()
        .map(|region| region.devices.len())
        .sum();
    if device_count == 0 {
        return TopologyView {
            config_path: display,
            state: "empty".to_string(),
            message: Some("没有设备".to_string()),
            regions: topology
                .regions
                .iter()
                .map(|region| RegionView {
                    name: region.name.clone(),
                    devices: Vec::new(),
                })
                .collect(),
        };
    }
    view_from_topology(&display, &topology, &TailscaleDirectory::read())
}

fn view_from_topology(
    config_path: &str,
    topology: &Topology,
    directory: &TailscaleDirectory,
) -> TopologyView {
    TopologyView {
        config_path: config_path.to_string(),
        state: "ok".to_string(),
        message: None,
        regions: topology
            .regions
            .iter()
            .map(|region| RegionView {
                name: region.name.clone(),
                devices: region
                    .devices
                    .iter()
                    .map(|device| DeviceView {
                        name: device.name.clone(),
                        host: device.host.clone(),
                        port: device.port,
                        user: device.user.clone(),
                        mobile: device.mobile,
                        presence: presence_label(device, directory).to_string(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn presence_label(device: &Device, directory: &TailscaleDirectory) -> &'static str {
    let Some(address) = &device.tailscale_address else {
        return "none";
    };
    match directory.presence(address) {
        AddressPresence::Online => "online",
        AddressPresence::Offline => "offline",
        AddressPresence::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_points_at_the_example() {
        let dir = std::env::temp_dir().join(format!(
            "my-terminal-missing-topology-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let view = load_topology_from_dir(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(view.state, "missing");
        let message = view.message.expect("hint");
        assert!(message.contains("topology.example.json"), "{message}");
        assert!(message.contains(TOPOLOGY_FILE_NAME), "{message}");
        assert!(view.regions.is_empty());
    }

    #[test]
    fn empty_regions_say_there_are_no_devices() {
        let dir =
            std::env::temp_dir().join(format!("my-terminal-empty-topology-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(TOPOLOGY_FILE_NAME), r#"{"regions":[]}"#).unwrap();
        let view = load_topology_from_dir(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(view.state, "empty");
        assert_eq!(view.message.as_deref(), Some("没有设备"));
    }

    #[test]
    fn unknown_fields_do_not_fail_parsing() {
        let json = r#"{"regions":[{"name":"r","note":"x","devices":[{"name":"a","host":"192.0.2.1","user":"me","extra":1}]}]}"#;
        let topology = parse_topology(json).expect("unknown fields are ignored");
        assert_eq!(topology.regions[0].devices[0].name, "a");
    }

    #[test]
    fn broken_json_is_an_error_view() {
        let dir =
            std::env::temp_dir().join(format!("my-terminal-bad-topology-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(TOPOLOGY_FILE_NAME), "{\n").unwrap();
        let view = load_topology_from_dir(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(view.state, "error");
        let message = view.message.expect("error");
        assert!(
            message.contains("行") || message.contains("line"),
            "{message}"
        );
    }
}

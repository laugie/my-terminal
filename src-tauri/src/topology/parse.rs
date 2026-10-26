use serde_json::{Map, Value};

use super::args::check_device_tokens;
use super::validate::validate_graph;

/// One topology file. Device names are unique across every region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topology {
    pub regions: Vec<Region>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub name: String,
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub identity_file: Option<String>,
    pub jump: Option<String>,
    pub mobile: bool,
    pub tailscale_address: Option<String>,
    /// JSON `type`. Any string is kept. Missing and non-string values are None.
    pub kind: Option<String>,
    /// Extra relays. Empty when the file has no `connections` array.
    pub connections: Vec<RelayConnection>,
}

/// One command relay through another device. `name` is unique on that device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayConnection {
    pub name: String,
    pub via: String,
    pub command: String,
    /// Target host for this route only. None uses the device `host`.
    pub host: Option<String>,
    /// Target port for this route only. None uses the device `port`.
    pub port: Option<u16>,
}

const FILE_LABEL: &str = "topology.json";

/// Parse a topology document.
///
/// Unknown object keys are ignored. A warning is written to stderr.
/// Syntax errors use `topology.json:<line>:<column>`.
pub fn parse_topology(json: &str) -> Result<Topology, String> {
    let value: Value = serde_json::from_str(json).map_err(syntax_error)?;
    let topology = build_topology(&value)?;
    for region in &topology.regions {
        for device in &region.devices {
            check_device_tokens(device)
                .map_err(|err| format!("区域「{}」设备「{}」：{err}", region.name, device.name))?;
        }
    }
    validate_graph(&topology)?;
    Ok(topology)
}

fn syntax_error(err: serde_json::Error) -> String {
    format!(
        "{FILE_LABEL}:{}:{} JSON 语法错误，行 {} 列 {}：{err}",
        err.line(),
        err.column(),
        err.line(),
        err.column()
    )
}

fn missing(field: &str) -> String {
    format!("{FILE_LABEL}:1:1 缺少必填字段 {field}")
}

fn build_topology(value: &Value) -> Result<Topology, String> {
    let root = value
        .as_object()
        .ok_or_else(|| format!("{FILE_LABEL}:1:1 根节点必须是对象"))?;
    warn_unknown(root, &["regions"], "");
    let Some(regions_value) = root.get("regions") else {
        return Err(missing("regions"));
    };
    if regions_value.is_null() {
        return Err(missing("regions"));
    }
    let regions = regions_value
        .as_array()
        .ok_or_else(|| format!("{FILE_LABEL}:1:1 字段 regions 必须是数组"))?;
    let mut parsed = Vec::with_capacity(regions.len());
    for region in regions {
        parsed.push(parse_region(region)?);
    }
    Ok(Topology { regions: parsed })
}

fn parse_region(value: &Value) -> Result<Region, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{FILE_LABEL}:1:1 区域必须是对象"))?;
    warn_unknown(object, &["name", "devices"], "区域.");
    let name = require_string(object, "name")?;
    reject_control(&name, "区域名")?;
    let Some(devices_value) = object.get("devices") else {
        return Err(missing("devices"));
    };
    let devices = devices_value
        .as_array()
        .ok_or_else(|| format!("{FILE_LABEL}:1:1 字段 devices 必须是数组"))?;
    let mut parsed = Vec::with_capacity(devices.len());
    for device in devices {
        parsed.push(parse_device(device, &name)?);
    }
    Ok(Region {
        name,
        devices: parsed,
    })
}

fn parse_device(value: &Value, region: &str) -> Result<Device, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{FILE_LABEL}:1:1 区域「{region}」中的设备必须是对象"))?;
    warn_unknown(
        object,
        &[
            "name",
            "host",
            "port",
            "user",
            "type",
            "jump",
            "mobile",
            "tailscaleAddress",
            "identityFile",
            "connections",
        ],
        &format!("区域「{region}」."),
    );
    let name = require_string(object, "name").map_err(|err| format!("区域「{region}」：{err}"))?;
    reject_control(&name, "设备名")?;
    let host = require_string(object, "host")
        .map_err(|err| format!("区域「{region}」设备「{name}」：{err}"))?;
    let user = require_string(object, "user")
        .map_err(|err| format!("区域「{region}」设备「{name}」：{err}"))?;
    let port = parse_port(object.get("port"))
        .map_err(|err| format!("区域「{region}」设备「{name}」：{err}"))?;
    let jump = optional_string(object, "jump")?;
    let mobile = parse_mobile(object.get("mobile"))
        .map_err(|err| format!("区域「{region}」设备「{name}」：{err}"))?;
    let tailscale_address = optional_string(object, "tailscaleAddress")?;
    let identity_file = optional_string(object, "identityFile")?;
    let kind = parse_kind(object.get("type"));
    let connections = parse_connections(object.get("connections"), region, &name)?;
    Ok(Device {
        name,
        host,
        port,
        user,
        identity_file,
        jump,
        mobile,
        tailscale_address,
        kind,
        connections,
    })
}

fn parse_connections(
    value: Option<&Value>,
    region: &str,
    device: &str,
) -> Result<Vec<RelayConnection>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let items = value.as_array().ok_or_else(|| {
        format!("{FILE_LABEL}:1:1 区域「{region}」设备「{device}」：字段 connections 类型无效")
    })?;
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        parsed.push(parse_connection(item, region, device)?);
    }
    Ok(parsed)
}

fn parse_connection(value: &Value, region: &str, device: &str) -> Result<RelayConnection, String> {
    let object = value.as_object().ok_or_else(|| {
        format!("{FILE_LABEL}:1:1 区域「{region}」设备「{device}」：连接必须是对象")
    })?;
    warn_unknown(
        object,
        &["name", "via", "command", "host", "port"],
        &format!("区域「{region}」设备「{device}」.connections."),
    );
    let name = require_string(object, "name").map_err(|err| place(region, device, err))?;
    reject_control(&name, "连接名")?;
    let via = require_string(object, "via").map_err(|err| place(region, device, err))?;
    reject_control(&via, "via")?;
    let command = require_string(object, "command").map_err(|err| place(region, device, err))?;
    super::relay::reject_relay_template(&command).map_err(|err| place(region, device, err))?;
    let host = optional_string(object, "host").map_err(|err| place(region, device, err))?;
    if let Some(host) = &host {
        reject_control(host, "连接 host").map_err(|err| place(region, device, err))?;
        if !super::relay::shell_safe_host(host) {
            return Err(place(
                region,
                device,
                format!("{FILE_LABEL}:1:1 连接「{name}」的 host 只能包含字母、数字和 . - _"),
            ));
        }
    }
    let port = match object.get("port") {
        None | Some(Value::Null) => None,
        value => Some(parse_port(value).map_err(|err| place(region, device, err))?),
    };
    Ok(RelayConnection {
        name,
        via,
        command,
        host,
        port,
    })
}

fn place(region: &str, device: &str, err: String) -> String {
    format!("区域「{region}」设备「{device}」：{err}")
}

/// Keep every string, including the empty string and unknown names.
/// Numbers, objects, and other non-strings are ignored.
fn parse_kind(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn require_string(object: &Map<String, Value>, field: &str) -> Result<String, String> {
    match object.get(field) {
        None | Some(Value::Null) => Err(missing(field)),
        Some(Value::String(text)) if text.is_empty() => Err(missing(field)),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(format!("{FILE_LABEL}:1:1 字段 {field} 类型无效")),
    }
}

fn optional_string(object: &Map<String, Value>, field: &str) -> Result<Option<String>, String> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.is_empty() => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("{FILE_LABEL}:1:1 字段 {field} 类型无效")),
    }
}

fn parse_port(value: Option<&Value>) -> Result<u16, String> {
    let Some(value) = value else {
        return Ok(22);
    };
    if value.is_null() {
        return Ok(22);
    }
    let Some(port) = value.as_u64() else {
        return Err(format!("{FILE_LABEL}:1:1 端口无效"));
    };
    if port == 0 || port > 65535 {
        return Err(format!("{FILE_LABEL}:1:1 端口无效"));
    }
    Ok(port as u16)
}

fn parse_mobile(value: Option<&Value>) -> Result<bool, String> {
    match value {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(_) => Err(format!("{FILE_LABEL}:1:1 字段 mobile 类型无效")),
    }
}

fn reject_control(value: &str, label: &str) -> Result<(), String> {
    if value.chars().any(|ch| ch.is_control()) {
        return Err(format!("{FILE_LABEL}:1:1 {label}不能包含控制字符"));
    }
    Ok(())
}

fn warn_unknown(object: &Map<String, Value>, known: &[&str], prefix: &str) {
    for key in object.keys() {
        if known.iter().any(|candidate| candidate == key) {
            continue;
        }
        eprintln!("{FILE_LABEL} 警告：忽略未知字段 {prefix}{key}");
    }
}

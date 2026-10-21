use std::collections::{HashMap, HashSet};

use super::parse::{Device, Topology};

pub(crate) fn validate_graph(topology: &Topology) -> Result<(), String> {
    reject_duplicate_names(topology)?;
    reject_missing_jumps(topology)?;
    reject_cycles(topology)?;
    reject_relay_edges(topology)?;
    Ok(())
}

/// v0.2: a relay `via` device cannot itself jump or relay.
fn reject_relay_edges(topology: &Topology) -> Result<(), String> {
    let by_name = index(topology);
    for region in &topology.regions {
        for device in &region.devices {
            let mut names = HashSet::new();
            for connection in &device.connections {
                if connection.name == "默认" {
                    return Err(format!(
                        "区域「{}」设备「{}」：连接名「默认」是保留名",
                        region.name, device.name
                    ));
                }
                if !names.insert(connection.name.as_str()) {
                    return Err(format!(
                        "区域「{}」设备「{}」：连接名「{}」重复",
                        region.name, device.name, connection.name
                    ));
                }
                if connection.via == device.name {
                    return Err(format!(
                        "区域「{}」设备「{}」：via 不能是自己",
                        region.name, device.name
                    ));
                }
                let Some(via) = by_name.get(connection.via.as_str()) else {
                    return Err(format!(
                        "区域「{}」设备「{}」：via 指向不存在的设备「{}」",
                        region.name, device.name, connection.via
                    ));
                };
                if via.jump.is_some() || !via.connections.is_empty() {
                    return Err(format!(
                        "区域「{}」设备「{}」：中转跳板机不能再有 jump",
                        region.name, device.name
                    ));
                }
            }
        }
    }
    Ok(())
}

fn reject_duplicate_names(topology: &Topology) -> Result<(), String> {
    let mut seen = HashSet::new();
    for region in &topology.regions {
        for device in &region.devices {
            if !seen.insert(device.name.as_str()) {
                return Err(format!(
                    "区域「{}」设备「{}」：名称重复",
                    region.name, device.name
                ));
            }
        }
    }
    Ok(())
}

fn reject_missing_jumps(topology: &Topology) -> Result<(), String> {
    let names: HashSet<&str> = topology
        .regions
        .iter()
        .flat_map(|region| region.devices.iter().map(|device| device.name.as_str()))
        .collect();
    for region in &topology.regions {
        for device in &region.devices {
            if let Some(target) = &device.jump {
                if !names.contains(target.as_str()) {
                    return Err(format!(
                        "区域「{}」设备「{}」：jump 指向不存在的设备「{target}」",
                        region.name, device.name
                    ));
                }
            }
        }
    }
    Ok(())
}

fn reject_cycles(topology: &Topology) -> Result<(), String> {
    let by_name = index(topology);
    for region in &topology.regions {
        for device in &region.devices {
            if let Some(path) = cycle_from(&device.name, &by_name) {
                return Err(format!(
                    "区域「{}」设备「{}」：jump 循环 {}",
                    region.name,
                    device.name,
                    path.join(" → ")
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn index<'a>(topology: &'a Topology) -> HashMap<&'a str, &'a Device> {
    let mut by_name = HashMap::new();
    for region in &topology.regions {
        for device in &region.devices {
            by_name.insert(device.name.as_str(), device);
        }
    }
    by_name
}

fn cycle_from<'a>(start: &'a str, by_name: &HashMap<&'a str, &'a Device>) -> Option<Vec<String>> {
    let mut stack: Vec<&str> = Vec::new();
    let mut cursor = start;
    loop {
        if let Some(pos) = stack.iter().position(|name| *name == cursor) {
            let mut path: Vec<String> = stack[pos..]
                .iter()
                .map(|name| (*name).to_string())
                .collect();
            path.push(cursor.to_string());
            return Some(path);
        }
        let device = by_name.get(cursor)?;
        let next = device.jump.as_deref()?;
        if !by_name.contains_key(next) {
            return None;
        }
        stack.push(cursor);
        cursor = next;
        if stack.len() > by_name.len() + 1 {
            return None;
        }
    }
}

pub(crate) fn region_of<'a>(topology: &'a Topology, device_name: &str) -> &'a str {
    topology
        .regions
        .iter()
        .find(|region| {
            region
                .devices
                .iter()
                .any(|device| device.name == device_name)
        })
        .map(|region| region.name.as_str())
        .unwrap_or("")
}

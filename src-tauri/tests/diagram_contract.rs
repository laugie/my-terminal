// #20 Rust 契约：src-tauri/tests/diagram_contract.rs（fixture 放 tests/fixtures/）。
// 期望：Device 增加 kind: Option<String>（JSON 字段 "type"，任意字符串原样保留，非字符串忽略）；
//       DeviceView 增加 type（Option<String>）与 jump（Option<String>），供前端画图标与连线。
use my_terminal_lib::topology::{load_topology_from_dir, parse_topology};

const DIAGRAM: &str = include_str!("fixtures/topology.diagram.json");
const VALID_16: &str = include_str!("fixtures/topology.valid.json");

#[test]
fn optional_type_is_parsed_and_unknown_values_do_not_error() {
    let t = parse_topology(DIAGRAM).expect("含 type 的配置应能加载");
    let all: Vec<_> = t.regions.iter().flat_map(|r| r.devices.iter()).collect();
    let nas = all.iter().find(|d| d.name == "示例NAS").unwrap();
    assert_eq!(nas.kind.as_deref(), Some("nas"));
    let printer = all.iter().find(|d| d.name == "打印机").unwrap();
    assert_eq!(printer.kind.as_deref(), Some("toaster"), "未知取值保留，交前端回退");
    assert!(all.iter().find(|d| d.name == "office-router").unwrap().kind.is_none());
}

#[test]
fn non_string_type_is_ignored_not_fatal() {
    let j = r#"{"regions":[{"name":"r","devices":[{"name":"a","host":"192.0.2.1","user":"me","type":5}]}]}"#;
    let t = parse_topology(j).expect("type 非字符串也不报错");
    assert!(t.regions[0].devices[0].kind.is_none());
}

#[test]
fn old_config_without_type_still_loads() {
    let t = parse_topology(VALID_16).unwrap();
    assert!(t.regions.iter().flat_map(|r| &r.devices).all(|d| d.kind.is_none()));
}

#[test]
fn view_exposes_type_and_jump_for_drawing() {
    let dir = std::env::temp_dir().join(format!("mt20-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("topology.json"), DIAGRAM).unwrap();
    let view = load_topology_from_dir(&dir);
    assert_eq!(view.state, "ok");
    let json = serde_json::to_value(&view).unwrap();
    let devices: Vec<_> = json["regions"].as_array().unwrap().iter()
        .flat_map(|r| r["devices"].as_array().unwrap().clone()).collect();
    let app = devices.iter().find(|d| d["name"] == "app-server").unwrap();
    assert_eq!(app["jump"], "bastion");
    let nas = devices.iter().find(|d| d["name"] == "示例NAS").unwrap();
    assert_eq!(nas["type"], "nas");
    // 跳板之外的连接字段（identityFile 等）仍不下发前端
    assert!(app.get("identityFile").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

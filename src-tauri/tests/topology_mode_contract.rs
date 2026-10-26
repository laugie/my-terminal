// #35 失败测试：拓扑视图模式（3D 星球 / 平面图）保存在 settings.topologyMode。
// 默认 3D 不写字段；未知值按 3D 处理，不当作配置损坏。
use std::fs;
use std::path::PathBuf;

use my_terminal_lib::config::{load_config, JsonHostStore, TOPOLOGY_MODES};

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "my-terminal-topology-mode-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn modes_match_the_frontend() {
    assert_eq!(TOPOLOGY_MODES, ["3d", "2d"]);
}

#[test]
fn flat_view_is_written_and_3d_removes_the_field() {
    let dir = temp_dir("persist");
    let mut store = JsonHostStore::open(&dir);
    assert_eq!(store.appearance_view().topology_mode, "3d");
    store.set_topology_mode("2d").unwrap();
    let text = fs::read_to_string(dir.join("config.json")).unwrap();
    assert!(text.contains("\"topologyMode\": \"2d\""), "{text}");
    assert_eq!(JsonHostStore::open(&dir).appearance_view().topology_mode, "2d");
    store.set_topology_mode("3d").unwrap();
    let text = fs::read_to_string(dir.join("config.json")).unwrap();
    assert!(!text.contains("topologyMode"), "{text}");
    assert!(store.set_topology_mode("4d").is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_mode_is_3d_and_not_corruption() {
    let dir = temp_dir("bad");
    for bad in ["\"4d\"", "3", "null", "{}"] {
        fs::write(
            dir.join("config.json"),
            format!(r#"{{"version":1,"manualHosts":[],"settings":{{"topologyMode":{bad}}}}}"#),
        )
        .unwrap();
        let config = load_config(&dir).unwrap();
        assert_eq!(config.notice, None, "{bad}");
        assert_eq!(JsonHostStore::open(&dir).appearance_view().topology_mode, "3d", "{bad}");
    }
    let _ = fs::remove_dir_all(&dir);
}

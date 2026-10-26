// #33 失败测试：界面缩放保存在 config.json 的 settings.uiZoom（整数百分比）。
// 100% 不写这个字段，旧文件保持原样；值不合法时按 100% 处理，不当作配置损坏。
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use my_terminal_lib::config::{load_config, save_config, Config, JsonHostStore, ZOOM_LEVELS};
use my_terminal_lib::hosts::NewManualHost;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "my-terminal-zoom-{label}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn with_host(dir: &Path) {
    let mut store = JsonHostStore::open(dir);
    store
        .add(NewManualHost {
            name: "box".to_string(),
            host: "192.0.2.7".to_string(),
            user: Some("me".to_string()),
            port: 22,
        })
        .unwrap();
}

fn config_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("config.json")).unwrap()
}

fn no_corrupt_backup(dir: &Path) -> bool {
    fs::read_dir(dir)
        .unwrap()
        .all(|entry| !entry.unwrap().file_name().to_string_lossy().contains("corrupt"))
}

#[test]
fn ladder_matches_the_frontend() {
    assert_eq!(ZOOM_LEVELS, [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200]);
}

#[test]
fn missing_field_is_100_percent() {
    let dir = TempDir::new("missing");
    with_host(&dir);
    assert_eq!(load_config(&dir).unwrap().settings.ui_zoom, None);
    assert_eq!(JsonHostStore::open(&dir).ui_zoom_view().percent, 100);
}

#[test]
fn zoom_survives_restart_and_keeps_manual_hosts() {
    let dir = TempDir::new("persist");
    with_host(&dir);
    let mut store = JsonHostStore::open(&dir);
    store.set_ui_zoom(125).unwrap();
    assert_eq!(store.ui_zoom_view().percent, 125);
    let text = config_text(&dir);
    assert!(text.contains("\"uiZoom\": 125"), "{text}");

    let reopened = JsonHostStore::open(&dir);
    assert_eq!(reopened.ui_zoom_view().percent, 125);
    assert_eq!(reopened.list().len(), 1, "手动服务器保留");
}

#[test]
fn back_to_100_removes_the_field_and_restores_the_file() {
    let dir = TempDir::new("reset");
    with_host(&dir);
    let before = config_text(&dir);
    let mut store = JsonHostStore::open(&dir);
    store.set_ui_zoom(150).unwrap();
    store.set_ui_zoom(100).unwrap();
    assert_eq!(config_text(&dir), before, "100% 时文件与改之前一致");
    assert!(!config_text(&dir).contains("uiZoom"));
}

#[test]
fn bad_values_mean_100_and_are_not_corruption() {
    for bad in ["\"big\"", "1.5", "112", "-5", "0", "null", "true", "{}", "70000"] {
        let dir = TempDir::new("bad");
        fs::write(
            dir.join("config.json"),
            format!(
                r#"{{"version":1,"manualHosts":[{{"id":"manual-1","name":"box","host":"192.0.2.7","user":"me","port":22}}],"settings":{{"uiZoom":{bad}}}}}"#
            ),
        )
        .unwrap();
        let config = load_config(&dir).unwrap();
        assert_eq!(config.settings.ui_zoom, None, "{bad}");
        assert_eq!(config.notice, None, "{bad} 不能当作损坏");
        assert_eq!(config.manual_hosts.len(), 1, "{bad}");
        assert!(no_corrupt_backup(&dir), "{bad}");
        assert_eq!(JsonHostStore::open(&dir).ui_zoom_view().percent, 100, "{bad}");
    }
}

#[test]
fn off_ladder_set_is_rejected_and_file_unchanged() {
    let dir = TempDir::new("off");
    with_host(&dir);
    let before = config_text(&dir);
    let mut store = JsonHostStore::open(&dir);
    for bad in [0, 49, 112, 201, 1000] {
        assert!(store.set_ui_zoom(bad).is_err(), "{bad}");
    }
    assert_eq!(config_text(&dir), before);
    assert_eq!(store.ui_zoom_view().percent, 100);
}

#[test]
fn newer_config_is_read_only_for_zoom() {
    let dir = TempDir::new("newer");
    let text = r#"{"version":99,"manualHosts":[],"settings":{"uiZoom":110}}"#;
    fs::write(dir.join("config.json"), text).unwrap();
    let mut store = JsonHostStore::open(&dir);
    let view = store.ui_zoom_view();
    assert_eq!(view.percent, 110);
    assert!(view.read_only);
    assert!(view.message.is_some());
    let err = store.set_ui_zoom(125).unwrap_err();
    assert!(err.contains("更新版本"), "{err}");
    assert_eq!(config_text(&dir), text, "不改更新版本的文件");
}

#[test]
fn save_config_round_trips_the_field() {
    let dir = TempDir::new("roundtrip");
    let mut config = Config::default();
    config.settings.ui_zoom = Some(80);
    save_config(&dir, &config).unwrap();
    assert_eq!(load_config(&dir).unwrap().settings.ui_zoom, Some(80));
}

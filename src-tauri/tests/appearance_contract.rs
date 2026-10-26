// #35 失败测试：终端配色和字体保存在 config.json 的 settings.terminalTheme / terminalFont。
// 默认值不写字段，旧文件保持原样；未知的值按默认处理，不当作配置损坏。
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use my_terminal_lib::config::{load_config, JsonHostStore, TERMINAL_FONTS, TERMINAL_THEMES};
use my_terminal_lib::hosts::NewManualHost;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "my-terminal-appearance-{label}-{}-{n}",
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

#[test]
fn lists_match_the_frontend() {
    assert_eq!(
        TERMINAL_THEMES,
        [
            "midnight",
            "deep-sea",
            "one-dark",
            "dracula",
            "tokyo-night",
            "nord",
            "solarized-dark",
            "paper"
        ]
    );
    assert_eq!(
        TERMINAL_FONTS,
        [
            "jetbrains-mono",
            "system",
            "sf-mono",
            "menlo",
            "cascadia-mono",
            "consolas",
            "fira-code",
            "meslo-nerd"
        ]
    );
}

#[test]
fn missing_fields_are_the_defaults() {
    let dir = TempDir::new("missing");
    with_host(&dir);
    let view = JsonHostStore::open(&dir).appearance_view();
    assert_eq!(view.theme, "midnight");
    assert_eq!(view.font, "jetbrains-mono");
    assert!(!view.read_only);
}

#[test]
fn choices_survive_restart_and_keep_manual_hosts() {
    let dir = TempDir::new("persist");
    with_host(&dir);
    let mut store = JsonHostStore::open(&dir);
    store.set_appearance(Some("dracula"), None).unwrap();
    store.set_appearance(None, Some("menlo")).unwrap();
    let text = config_text(&dir);
    assert!(text.contains("\"terminalTheme\": \"dracula\""), "{text}");
    assert!(text.contains("\"terminalFont\": \"menlo\""), "{text}");

    let reopened = JsonHostStore::open(&dir);
    let view = reopened.appearance_view();
    assert_eq!(view.theme, "dracula");
    assert_eq!(view.font, "menlo");
    assert_eq!(reopened.list().len(), 1, "手动服务器保留");
}

#[test]
fn back_to_defaults_restores_the_file() {
    let dir = TempDir::new("reset");
    with_host(&dir);
    let before = config_text(&dir);
    let mut store = JsonHostStore::open(&dir);
    store.set_appearance(Some("nord"), Some("consolas")).unwrap();
    store
        .set_appearance(Some("midnight"), Some("jetbrains-mono"))
        .unwrap();
    assert_eq!(config_text(&dir), before, "默认值时文件与改之前一致");
}

#[test]
fn unknown_values_mean_defaults_and_are_not_corruption() {
    for bad in ["\"neon\"", "\"\"", "1", "null", "true", "{}", "[\"nord\"]"] {
        let dir = TempDir::new("bad");
        fs::write(
            dir.join("config.json"),
            format!(
                r#"{{"version":1,"manualHosts":[{{"id":"manual-1","name":"box","host":"192.0.2.7","user":"me","port":22}}],"settings":{{"terminalTheme":{bad},"terminalFont":{bad}}}}}"#
            ),
        )
        .unwrap();
        let config = load_config(&dir).unwrap();
        assert_eq!(config.settings.terminal_theme, None, "{bad}");
        assert_eq!(config.settings.terminal_font, None, "{bad}");
        assert_eq!(config.notice, None, "{bad} 不能当作损坏");
        assert_eq!(config.manual_hosts.len(), 1, "{bad}");
        let view = JsonHostStore::open(&dir).appearance_view();
        assert_eq!((view.theme.as_str(), view.font.as_str()), ("midnight", "jetbrains-mono"));
    }
}

#[test]
fn unknown_ids_are_rejected_and_file_unchanged() {
    let dir = TempDir::new("reject");
    with_host(&dir);
    let before = config_text(&dir);
    let mut store = JsonHostStore::open(&dir);
    assert!(store.set_appearance(Some("neon"), None).is_err());
    assert!(store.set_appearance(None, Some("comic-sans")).is_err());
    assert!(store.set_appearance(Some("nord"), Some("comic-sans")).is_err(), "整次拒绝");
    assert_eq!(config_text(&dir), before);
}

#[test]
fn newer_config_is_read_only() {
    let dir = TempDir::new("newer");
    let text = r#"{"version":99,"manualHosts":[],"settings":{"terminalTheme":"nord"}}"#;
    fs::write(dir.join("config.json"), text).unwrap();
    let mut store = JsonHostStore::open(&dir);
    let view = store.appearance_view();
    assert_eq!(view.theme, "nord");
    assert!(view.read_only);
    assert!(view.message.is_some());
    assert!(store.set_appearance(Some("dracula"), None).is_err());
    assert_eq!(config_text(&dir), text, "不改更新版本的文件");
}

#[test]
fn auto_beautify_is_on_unless_false_is_written() {
    let dir = TempDir::new("auto");
    with_host(&dir);
    let before = config_text(&dir);
    let mut store = JsonHostStore::open(&dir);
    assert!(store.appearance_view().auto_beautify, "默认开启");
    store.set_auto_beautify(false).unwrap();
    assert!(config_text(&dir).contains("\"autoBeautify\": false"));
    assert!(!JsonHostStore::open(&dir).appearance_view().auto_beautify);
    store.set_auto_beautify(true).unwrap();
    assert_eq!(config_text(&dir), before, "开启时不写字段");

    for other in ["true", "0", "\"no\"", "null"] {
        fs::write(
            dir.join("config.json"),
            format!(r#"{{"version":1,"manualHosts":[],"settings":{{"autoBeautify":{other}}}}}"#),
        )
        .unwrap();
        let config = load_config(&dir).unwrap();
        assert_eq!(config.notice, None, "{other}");
        assert!(JsonHostStore::open(&dir).appearance_view().auto_beautify, "{other}");
    }
}

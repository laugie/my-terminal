use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use my_terminal_lib::config::{load_config, save_config, Config, JsonHostStore};
use my_terminal_lib::hosts::NewManualHost;

fn temp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "my-terminal-config-{label}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        Self(temp_dir(label))
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample(name: &str, host: &str, port: u16) -> NewManualHost {
    NewManualHost {
        name: name.to_string(),
        host: host.to_string(),
        user: Some("user".to_string()),
        port,
    }
}

fn corrupt_backups(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("config.json.corrupt-") {
            found.push(entry.path());
        }
    }
    found.sort();
    found
}

#[test]
fn missing_file_gives_defaults() {
    let dir = TempDir::new("missing");
    let loaded = load_config(&dir).unwrap();
    assert_eq!(loaded, Config::default());
    assert!(!dir.join("config.json").exists());
}

#[test]
fn manual_hosts_survive_restart() {
    let dir = TempDir::new("restart");
    {
        let mut store = JsonHostStore::open(&dir);
        store.add(sample("lab", "host-a.example", 2222)).unwrap();
    }
    let store = JsonHostStore::open(&dir);
    let list = store.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "lab");
    assert_eq!(list[0].host, "host-a.example");
    assert_eq!(list[0].user.as_deref(), Some("user"));
    assert_eq!(list[0].port, 2222);
}

#[test]
fn ids_stay_unique_after_restart() {
    let dir = TempDir::new("ids");
    let first = {
        let mut store = JsonHostStore::open(&dir);
        store.add(sample("lab", "host-a.example", 2222)).unwrap()
    };
    assert_eq!(first.id, "manual-1");

    let mut store = JsonHostStore::open(&dir);
    let second = store.add(sample("other", "host-a.example", 22)).unwrap();
    assert_eq!(second.id, "manual-2");
    let ids: Vec<_> = store
        .list()
        .into_iter()
        .map(|host| host.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["manual-1".to_string(), "manual-2".to_string()]);
    assert_ne!(ids[0], ids[1]);

    let seeded = TempDir::new("ids-max");
    fs::write(
        seeded.join("config.json"),
        r#"{"version":1,"manualHosts":[{"id":"manual-7","name":"lab","host":"host-a.example","user":"user","port":2222}],"settings":{}}"#,
    )
    .unwrap();
    let mut store = JsonHostStore::open(&seeded);
    let next = store.add(sample("other", "host-a.example", 22)).unwrap();
    assert_eq!(next.id, "manual-8");
    let ids: Vec<_> = store
        .list()
        .into_iter()
        .map(|host| host.id)
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], "manual-7");
    assert_eq!(ids[1], "manual-8");
}

#[test]
fn corrupt_file_is_backed_up_and_defaults_used() {
    let dir = TempDir::new("corrupt");
    fs::write(dir.join("config.json"), "{ not json").unwrap();
    let loaded = load_config(&dir).unwrap();
    assert_eq!(loaded.version, Config::default().version);
    assert_eq!(loaded.manual_hosts, Config::default().manual_hosts);
    assert_eq!(loaded.settings, Config::default().settings);
    assert_eq!(loaded.notice.as_deref(), Some("配置文件损坏，已备份并重置"));

    let backups = corrupt_backups(&dir);
    assert_eq!(backups.len(), 1);
    let name = backups[0].file_name().unwrap().to_string_lossy();
    let stamp = name.strip_prefix("config.json.corrupt-").unwrap();
    assert_eq!(stamp.len(), 16);
    assert!(stamp.as_bytes()[8] == b'T' && stamp.ends_with('Z'));
    assert!(stamp.as_bytes()[..8].iter().all(u8::is_ascii_digit));
    assert!(stamp.as_bytes()[9..15].iter().all(u8::is_ascii_digit));
    assert_eq!(fs::read(&backups[0]).unwrap(), b"{ not json");
}

#[test]
fn saved_json_has_version_and_no_secret_fields() {
    let dir = TempDir::new("secrets");
    let mut store = JsonHostStore::open(&dir);
    store.add(sample("lab", "host-a.example", 2222)).unwrap();
    let text = fs::read_to_string(dir.join("config.json")).unwrap();
    assert!(text.contains("\"version\""));
    let lower = text.to_lowercase();
    for word in [
        "password",
        "passphrase",
        "private_key",
        "privatekey",
        "secret",
        "token",
        "identity",
    ] {
        assert!(!lower.contains(word), "{word} appeared in {text}");
    }
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["settings"], serde_json::json!({}));
    assert!(value.get("manual_hosts").is_none());
    assert_eq!(value["manualHosts"][0]["host"], "host-a.example");
    assert_eq!(value["manualHosts"][0]["port"], 2222);
    assert!(text.contains("\n  \"version\": 1"));
}

#[test]
fn unknown_fields_and_newer_version_do_not_crash() {
    let dir = TempDir::new("newer");
    let original = r#"{
  "version": 99,
  "extra": true,
  "manualHosts": [
    {
      "id": "manual-4",
      "name": "lab",
      "host": "host-a.example",
      "user": "user",
      "port": 2222,
      "futureField": "later"
    }
  ],
  "settings": { "theme": "later" },
  "future": { "keep": true }
}
"#;
    fs::write(dir.join("config.json"), original).unwrap();
    let before = fs::read(dir.join("config.json")).unwrap();

    let loaded = load_config(&dir).unwrap();
    assert_eq!(loaded.manual_hosts.len(), 1);
    assert_eq!(loaded.manual_hosts[0].host, "host-a.example");
    assert_eq!(loaded.manual_hosts[0].port, 2222);
    assert_eq!(loaded.notice.as_deref(), Some("配置来自更新版本"));
    assert!(loaded.read_only);
    assert_eq!(fs::read(dir.join("config.json")).unwrap(), before);

    let mut store = JsonHostStore::open(&dir);
    assert_eq!(store.list().len(), 1);
    let add_err = store
        .add(sample("other", "host-a.example", 22))
        .unwrap_err();
    assert_eq!(add_err, "配置来自更新版本，无法保存修改");
    let remove_err = store.remove("manual-4").unwrap_err();
    assert_eq!(remove_err, "配置来自更新版本，无法保存修改");
    assert_eq!(store.list().len(), 1);
    assert_eq!(store.list()[0].id, "manual-4");
    assert_eq!(fs::read(dir.join("config.json")).unwrap(), before);
}

#[test]
fn write_is_atomic_no_tmp_left() {
    let dir = TempDir::new("atomic");
    save_config(&dir, &Config::default()).unwrap();
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        names.push(entry.unwrap().file_name().to_string_lossy().into_owned());
    }
    names.sort();
    assert_eq!(names, vec!["config.json".to_string()]);
}

#[test]
fn add_and_remove_fail_when_write_fails() {
    let dir = TempDir::new("unwritable");
    let mut store = JsonHostStore::open(&dir);
    store.set_save_hook(|_, _| Err("无法保存配置".to_string()));
    let err = store
        .add(sample("lab", "host-a.example", 2222))
        .unwrap_err();
    assert_eq!(err, "无法保存配置");
    assert!(store.list().is_empty());
    assert!(!dir.join("config.json").exists());

    drop(store);
    let mut store = JsonHostStore::open(&dir);
    store.add(sample("lab", "host-a.example", 2222)).unwrap();
    let written = fs::read(dir.join("config.json")).unwrap();
    store.set_save_hook(|_, _| Err("无法保存配置".to_string()));
    let err = store.remove("manual-1").unwrap_err();
    assert_eq!(err, "无法保存配置");
    assert_eq!(store.list().len(), 1);
    assert_eq!(fs::read(dir.join("config.json")).unwrap(), written);
}

#[test]
fn failed_atomic_write_keeps_the_previous_file() {
    let dir = TempDir::new("keep");
    let mut config = Config::default();
    config
        .manual_hosts
        .push(my_terminal_lib::hosts::ManualHost {
            id: "manual-1".to_string(),
            name: "lab".to_string(),
            host: "host-a.example".to_string(),
            user: Some("user".to_string()),
            port: 2222,
        });
    save_config(&dir, &config).unwrap();
    let original = fs::read(dir.join("config.json")).unwrap();
    fs::create_dir(dir.join("config.json.tmp")).unwrap();

    let mut updated = config.clone();
    updated.manual_hosts.clear();
    let err = save_config(&dir, &updated).unwrap_err();
    assert_eq!(err.user_message(), "无法保存配置");
    assert_eq!(fs::read(dir.join("config.json")).unwrap(), original);
}

#[test]
fn corrupt_backups_keep_the_newest_five() {
    let dir = TempDir::new("prune");
    for i in 0..7 {
        fs::write(
            dir.join(format!("config.json.corrupt-20200101T00000{i}Z")),
            format!("old-{i}"),
        )
        .unwrap();
    }
    fs::write(dir.join("config.json"), b"{ not json").unwrap();
    let loaded = load_config(&dir).unwrap();
    assert_eq!(loaded.notice.as_deref(), Some("配置文件损坏，已备份并重置"));

    let backups = corrupt_backups(&dir);
    assert_eq!(backups.len(), 5);
    let contents: Vec<String> = backups
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert!(contents.iter().any(|text| text == "{ not json"));
    assert!(contents.iter().any(|text| text == "old-6"));
    assert!(contents
        .iter()
        .all(|text| text != "old-0" && text != "old-1"));
}

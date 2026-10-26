//! Local JSON config. Load and save take a directory so tests never touch
//! the real application-support path.
//!
//! The on-disk file is `<dir>/config.json`:
//! macOS `~/Library/Application Support/my-terminal/config.json`,
//! Windows `%APPDATA%\my-terminal\config.json`.
//! That directory is `path().config_dir().join("my-terminal")`, not the
//! Tauri identifier directory.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::hosts::{HostStore, ManualHost, NewManualHost};

pub const CONFIG_VERSION: u32 = 1;
pub const CONFIG_DIR_NAME: &str = "my-terminal";
pub const CONFIG_FILE_NAME: &str = "config.json";
const TEMP_FILE_NAME: &str = "config.json.tmp";
const CORRUPT_PREFIX: &str = "config.json.corrupt-";
const MAX_CORRUPT_BACKUPS: usize = 5;

pub const CORRUPT_NOTICE: &str = "配置文件损坏，已备份并重置";
pub const NEWER_VERSION_NOTICE: &str = "配置来自更新版本";
pub const NEWER_VERSION_WRITE_ERROR: &str = "配置来自更新版本，无法保存修改";
pub const SAVE_ERROR: &str = "无法保存配置";
const READ_ERROR_NOTICE: &str = "无法读取配置，已使用默认值";

/// `system_config_dir` is the directory from `path().config_dir()`.
pub fn config_dir_under(system_config_dir: &Path) -> PathBuf {
    system_config_dir.join(CONFIG_DIR_NAME)
}

/// Real config directory for this process.
///
/// macOS: `~/Library/Application Support/my-terminal`.
/// Windows: `%APPDATA%\my-terminal`.
pub fn resolve_app_config_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use tauri::Manager;
    let base = app.path().config_dir()?;
    Ok(config_dir_under(&base))
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Format(serde_json::Error),
    ReadOnly,
}

impl ConfigError {
    pub fn user_message(&self) -> String {
        match self {
            ConfigError::ReadOnly => NEWER_VERSION_WRITE_ERROR.to_string(),
            ConfigError::Io(_) | ConfigError::Format(_) => SAVE_ERROR.to_string(),
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError::Io(err)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(err: serde_json::Error) -> Self {
        ConfigError::Format(err)
    }
}

/// UI zoom steps in percent. The frontend `ZOOM_LEVELS` is the same list.
pub const ZOOM_LEVELS: [u16; 11] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200];

pub fn is_zoom_level(percent: u16) -> bool {
    ZOOM_LEVELS.contains(&percent)
}

/// Local settings. `login_lock_enabled` is omitted when false so an old
/// `settings: {}` file stays byte-compatible and means the lock is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default, skip_serializing_if = "is_false")]
    pub login_lock_enabled: bool,
    /// UI zoom in percent. None is 100% and is not written.
    #[serde(
        default,
        deserialize_with = "lenient_zoom",
        skip_serializing_if = "Option::is_none"
    )]
    pub ui_zoom: Option<u16>,
}

/// A value off the zoom ladder is read as 100%. It must not fail the whole
/// file, which would be treated as corrupt and reset.
fn lenient_zoom<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value
        .as_u64()
        .and_then(|number| u16::try_from(number).ok())
        .filter(|percent| *percent != 100 && is_zoom_level(*percent)))
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub manual_hosts: Vec<ManualHost>,
    #[serde(default)]
    pub settings: Settings,
    /// Session-only status-bar text. Never written to disk.
    #[serde(skip)]
    pub notice: Option<String>,
    /// A newer-version file is loaded but not written back.
    #[serde(skip)]
    pub read_only: bool,
    /// `add` / `remove` return this while `read_only` is set.
    #[serde(skip)]
    pub write_error: Option<String>,
}

fn current_version() -> u32 {
    CONFIG_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            manual_hosts: Vec::new(),
            settings: Settings::default(),
            notice: None,
            read_only: false,
            write_error: None,
        }
    }
}

/// Load `config.json` from `dir`, creating `dir` when it is missing.
///
/// A missing file is the default config. A file that cannot be parsed is
/// renamed to `config.json.corrupt-<UTC stamp>` (content unchanged), then
/// replaced with the default document. A higher `version` is loaded read-only
/// and left untouched on disk.
pub fn load_config(dir: &Path) -> Result<Config, ConfigError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(CONFIG_FILE_NAME);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(err) => return Err(err.into()),
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return reset_corrupt(dir, &path),
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return reset_corrupt(dir, &path),
    };
    if let Some(version) = value.get("version").and_then(|v| v.as_u64()) {
        if version > u64::from(CONFIG_VERSION) {
            return Ok(mark_newer(value, version));
        }
    }
    match serde_json::from_value::<Config>(value) {
        Ok(config) => Ok(config),
        Err(_) => reset_corrupt(dir, &path),
    }
}

/// Set `settings.loginLockEnabled` and write `config.json` atomically.
///
/// Other fields, including manual hosts, are loaded and written back.
/// A newer-version file is left untouched.
pub fn set_login_lock_enabled(dir: &Path, enabled: bool) -> Result<(), ConfigError> {
    let mut config = load_config(dir)?;
    if config.read_only || config.version > CONFIG_VERSION {
        return Err(ConfigError::ReadOnly);
    }
    config.settings.login_lock_enabled = enabled;
    config.notice = None;
    save_config(dir, &config)
}

/// Pretty-print `config` as UTF-8 and replace `config.json` atomically.
pub fn save_config(dir: &Path, config: &Config) -> Result<(), ConfigError> {
    if config.read_only || config.version > CONFIG_VERSION {
        return Err(ConfigError::ReadOnly);
    }
    fs::create_dir_all(dir)?;
    let bytes = serialize_config(config)?;
    atomic_write(dir, &bytes)
}

fn serialize_config(config: &Config) -> Result<Vec<u8>, ConfigError> {
    let mut bytes = serde_json::to_vec_pretty(config)?;
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

/// `config.json.tmp` → fsync → rename. A failed write leaves the previous file.
fn atomic_write(dir: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let dest = dir.join(CONFIG_FILE_NAME);
    let tmp = dir.join(TEMP_FILE_NAME);
    write_temp(&tmp, bytes)?;
    if let Err(err) = replace_file(&tmp, &dest) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    sync_dir(dir);
    Ok(())
}

fn write_temp(tmp: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let result = (|| -> Result<(), ConfigError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

fn replace_file(tmp: &Path, dest: &Path) -> Result<(), ConfigError> {
    match fs::rename(tmp, dest) {
        Ok(()) => Ok(()),
        Err(first) if !dest.exists() => Err(first.into()),
        Err(first) => {
            // Older Windows `fs::rename` refuses to replace an existing file.
            // Move the live file aside and put it back if the new name fails.
            let backup = dest.with_file_name("config.json.replacing");
            if backup.exists() {
                return Err(first.into());
            }
            if let Err(move_err) = fs::rename(dest, &backup) {
                return Err(move_err.into());
            }
            if let Err(second) = fs::rename(tmp, dest) {
                let _ = fs::rename(&backup, dest);
                return Err(second.into());
            }
            let _ = fs::remove_file(&backup);
            Ok(())
        }
    }
}

fn sync_dir(dir: &Path) {
    if let Ok(file) = File::open(dir) {
        let _ = file.sync_all();
    }
}

fn reset_corrupt(dir: &Path, path: &Path) -> Result<Config, ConfigError> {
    backup_corrupt(dir, path)?;
    let mut config = Config::default();
    save_config(dir, &config)?;
    config.notice = Some(CORRUPT_NOTICE.to_string());
    Ok(config)
}

fn backup_corrupt(dir: &Path, path: &Path) -> Result<(), ConfigError> {
    let dest = unique_backup_path(dir, SystemTime::now());
    fs::rename(path, &dest)?;
    let _ = prune_corrupt_backups(dir);
    Ok(())
}

fn unique_backup_path(dir: &Path, now: SystemTime) -> PathBuf {
    let base = format!("{CORRUPT_PREFIX}{}", utc_compact_stamp(now));
    let first = dir.join(&base);
    if !first.exists() {
        return first;
    }
    let mut n = 2u32;
    loop {
        let candidate = dir.join(format!("{base}-{n}"));
        if !candidate.exists() || n == u32::MAX {
            return candidate;
        }
        n += 1;
    }
}

fn prune_corrupt_backups(dir: &Path) -> std::io::Result<()> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(CORRUPT_PREFIX) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    if paths.len() > MAX_CORRUPT_BACKUPS {
        let remove_count = paths.len() - MAX_CORRUPT_BACKUPS;
        for path in paths.into_iter().take(remove_count) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn mark_newer(value: serde_json::Value, version: u64) -> Config {
    let mut config = serde_json::from_value::<Config>(value.clone()).unwrap_or_else(|_| Config {
        manual_hosts: recoverable_hosts(&value),
        ..Config::default()
    });
    config.version = u32::try_from(version).unwrap_or(u32::MAX);
    if config.version <= CONFIG_VERSION {
        config.version = CONFIG_VERSION.saturating_add(1);
    }
    config.read_only = true;
    config.notice = Some(NEWER_VERSION_NOTICE.to_string());
    config.write_error = Some(NEWER_VERSION_WRITE_ERROR.to_string());
    config
}

fn recoverable_hosts(value: &serde_json::Value) -> Vec<ManualHost> {
    value
        .get("manualHosts")
        .and_then(|hosts| hosts.as_array())
        .map(|hosts| {
            hosts
                .iter()
                .filter_map(|host| serde_json::from_value(host.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn utc_compact_stamp(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = tod / 3_600;
    let minute = (tod % 3_600) / 60;
    let second = tod % 60;
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// Howard Hinnant's `civil_from_days` (public domain).
fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year_base = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if month <= 2 { year_base + 1 } else { year_base };
    (year as i32, month, day)
}

type SaveHook = Box<dyn Fn(&Path, &[u8]) -> Result<(), String> + Send>;

/// UI zoom as the settings row shows it. `message` explains a failed save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoomView {
    pub percent: u16,
    pub read_only: bool,
    pub message: Option<String>,
}

/// In-memory view of the login-lock switch. `message` is a config notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginLockView {
    pub enabled: bool,
    pub read_only: bool,
    pub message: Option<String>,
}

/// Manual servers stored in `config.json`. `add` and `remove` write immediately.
pub struct JsonHostStore {
    dir: PathBuf,
    config: Config,
    save_hook: Option<SaveHook>,
}

impl JsonHostStore {
    pub fn open(dir: &Path) -> Self {
        match load_config(dir) {
            Ok(config) => Self::from_parts(dir, config),
            Err(_) => Self::from_parts(
                dir,
                Config {
                    notice: Some(READ_ERROR_NOTICE.to_string()),
                    read_only: true,
                    write_error: Some(SAVE_ERROR.to_string()),
                    ..Config::default()
                },
            ),
        }
    }

    fn from_parts(dir: &Path, config: Config) -> Self {
        Self {
            dir: dir.to_path_buf(),
            config,
            save_hook: None,
        }
    }

    /// Send later saves through `write` instead of the real atomic write.
    /// A function that returns `Err` simulates a directory that cannot be written.
    pub fn set_save_hook(
        &mut self,
        write: impl Fn(&Path, &[u8]) -> Result<(), String> + Send + 'static,
    ) {
        self.save_hook = Some(Box::new(write));
    }

    pub fn notice(&self) -> Option<&str> {
        self.config.notice.as_deref()
    }

    pub fn login_lock_view(&self) -> LoginLockView {
        let message = if self.config.read_only {
            self.config
                .write_error
                .clone()
                .or_else(|| self.config.notice.clone())
        } else {
            self.config.notice.clone()
        };
        LoginLockView {
            enabled: self.config.settings.login_lock_enabled,
            read_only: self.config.read_only,
            message,
        }
    }

    /// Update the switch in this store and on disk together, so a later host
    /// save cannot write the previous flag back over it.
    pub fn set_login_lock_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.ensure_writable()?;
        let mut next = self.config.clone();
        next.settings.login_lock_enabled = enabled;
        next.version = CONFIG_VERSION;
        self.persist(&next)?;
        self.config = next;
        Ok(())
    }

    pub fn ui_zoom_view(&self) -> ZoomView {
        ZoomView {
            percent: self.config.settings.ui_zoom.unwrap_or(100),
            read_only: self.config.read_only,
            message: if self.config.read_only {
                self.config.write_error.clone()
            } else {
                None
            },
        }
    }

    /// Save a zoom level. 100% removes the field so the file is as before.
    pub fn set_ui_zoom(&mut self, percent: u16) -> Result<(), String> {
        if !is_zoom_level(percent) {
            return Err("缩放比例无效".to_string());
        }
        self.ensure_writable()?;
        let mut next = self.config.clone();
        next.settings.ui_zoom = (percent != 100).then_some(percent);
        next.version = CONFIG_VERSION;
        self.persist(&next)?;
        self.config = next;
        Ok(())
    }

    pub fn list(&self) -> Vec<ManualHost> {
        self.config.manual_hosts.clone()
    }

    pub fn add(&mut self, host: NewManualHost) -> Result<ManualHost, String> {
        self.ensure_writable()?;
        let stored = ManualHost {
            id: next_manual_id(&self.config.manual_hosts)?,
            name: host.name,
            host: host.host,
            user: host.user,
            port: host.port,
        };
        let mut next = self.config.clone();
        next.manual_hosts.push(stored.clone());
        next.version = CONFIG_VERSION;
        self.persist(&next)?;
        self.config = next;
        Ok(stored)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.ensure_writable()?;
        if !self.config.manual_hosts.iter().any(|host| host.id == id) {
            return Err("未找到该服务器".to_string());
        }
        let mut next = self.config.clone();
        next.manual_hosts.retain(|host| host.id != id);
        next.version = CONFIG_VERSION;
        self.persist(&next)?;
        self.config = next;
        Ok(())
    }

    fn ensure_writable(&self) -> Result<(), String> {
        if self.config.read_only {
            Err(self
                .config
                .write_error
                .clone()
                .unwrap_or_else(|| SAVE_ERROR.to_string()))
        } else {
            Ok(())
        }
    }

    fn persist(&self, config: &Config) -> Result<(), String> {
        if config.read_only || config.version > CONFIG_VERSION {
            return Err(config
                .write_error
                .clone()
                .unwrap_or_else(|| NEWER_VERSION_WRITE_ERROR.to_string()));
        }
        if let Some(hook) = &self.save_hook {
            let bytes = serialize_config(config).map_err(|_| SAVE_ERROR.to_string())?;
            return hook(&self.dir, &bytes);
        }
        save_config(&self.dir, config).map_err(|err| err.user_message())
    }
}

impl HostStore for JsonHostStore {
    fn list(&self) -> Vec<ManualHost> {
        JsonHostStore::list(self)
    }

    fn add(&mut self, host: NewManualHost) -> Result<ManualHost, String> {
        JsonHostStore::add(self, host)
    }

    fn remove(&mut self, id: &str) -> Result<(), String> {
        JsonHostStore::remove(self, id)
    }
}

fn next_manual_id(hosts: &[ManualHost]) -> Result<String, String> {
    let max = hosts
        .iter()
        .filter_map(|host| parse_manual_id(&host.id))
        .max()
        .unwrap_or(0);
    let next = max
        .checked_add(1)
        .ok_or_else(|| "无法分配服务器编号".to_string())?;
    Ok(format!("manual-{next}"))
}

fn parse_manual_id(id: &str) -> Option<u64> {
    let rest = id.strip_prefix("manual-")?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn config_dir_is_my_terminal_under_the_system_config_dir() {
        let dir = config_dir_under(Path::new("/tmp/app-support"));
        assert_eq!(dir, PathBuf::from("/tmp/app-support/my-terminal"));
    }

    #[test]
    fn utc_stamp_formats_known_days() {
        let epoch = utc_compact_stamp(UNIX_EPOCH);
        assert_eq!(epoch, "19700101T000000Z");
        let new_year_2024 = UNIX_EPOCH + Duration::from_secs(1_704_067_200);
        assert_eq!(utc_compact_stamp(new_year_2024), "20240101T000000Z");
        let oct_19_2024 = UNIX_EPOCH + Duration::from_secs(1_729_296_000);
        assert_eq!(utc_compact_stamp(oct_19_2024), "20241019T000000Z");
    }
}

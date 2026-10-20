use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::session::args::{build_ssh_args_with_options, SshOptions};

use super::parse::{Device, Topology};
use super::validate::{identity_directive, index, region_of};

/// Arguments for one device, without a generated ssh config.
///
/// Shape: `-t -l <user> -p <port> [-i <path>] [-J <outer>,...] -- <host>`.
pub fn ssh_args_for(topology: &Topology, device_name: &str) -> Result<Vec<String>, String> {
    let device =
        find(topology, device_name).ok_or_else(|| format!("没有名为「{device_name}」的设备"))?;
    let chain = jump_chain(topology, device)?;
    let jump = jump_spec(&chain)?;
    build_ssh_args_with_options(&SshOptions {
        host: device.host.clone(),
        user: Some(device.user.clone()),
        port: Some(device.port),
        identity_path: device.identity_file.clone(),
        jump,
    })
    .map_err(|err| {
        format!(
            "区域「{}」设备「{}」：{err}",
            region_of(topology, device_name),
            device.name
        )
    })
}

pub(crate) fn check_device_tokens(device: &Device) -> Result<(), String> {
    build_ssh_args_with_options(&SshOptions {
        host: device.host.clone(),
        user: Some(device.user.clone()),
        port: Some(device.port),
        identity_path: device.identity_file.clone(),
        jump: None,
    })
    .map(|_| ())
}

/// ssh argv for a device, plus a temp config when a jump hop names a key path.
pub struct SessionPlan {
    pub args: Vec<String>,
    pub temp_file: Option<TempSshConfig>,
}

/// One `0600` ssh config. Dropping an armed value deletes the file.
pub struct TempSshConfig {
    path: PathBuf,
    armed: bool,
}

impl TempSshConfig {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempSshConfig {
    fn drop(&mut self) {
        if self.armed && !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn plan_device_session(dir: &Path, device_name: &str) -> Result<SessionPlan, String> {
    let path = dir.join("topology.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "未找到拓扑配置。请创建 {}，格式可参考仓库根目录的 topology.example.json。",
                path.display()
            ));
        }
        Err(_) => return Err("无法读取拓扑配置".to_string()),
    };
    if text.len() > 1_048_576 {
        return Err("拓扑配置过大".to_string());
    }
    let topology = super::parse_topology(&text)?;
    let args = ssh_args_for(&topology, device_name)?;
    let device =
        find(&topology, device_name).ok_or_else(|| format!("没有名为「{device_name}」的设备"))?;
    let chain = jump_chain(&topology, device)?;
    if !chain.iter().any(|hop| hop.identity_file.is_some()) {
        return Ok(SessionPlan {
            args,
            temp_file: None,
        });
    }
    let temp_file = write_temp_config(&chain)?;
    let config_path = temp_file
        .path()
        .to_str()
        .filter(|text| !text.is_empty() && !text.starts_with('-'))
        .ok_or_else(|| "临时 ssh 配置路径无效".to_string())?
        .to_string();
    if config_path.chars().any(|ch| ch.is_control() || ch == '\n') {
        return Err("临时 ssh 配置路径无效".to_string());
    }
    let mut with_file = Vec::with_capacity(args.len() + 2);
    with_file.push("-F".to_string());
    with_file.push(config_path);
    with_file.extend(args);
    Ok(SessionPlan {
        args: with_file,
        temp_file: Some(temp_file),
    })
}

fn find<'a>(topology: &'a Topology, name: &str) -> Option<&'a Device> {
    index(topology).get(name).copied()
}

/// Outermost hop first. `T → B → C` becomes `[C, B]`.
fn jump_chain<'a>(topology: &'a Topology, target: &'a Device) -> Result<Vec<&'a Device>, String> {
    let mut toward_outer = Vec::new();
    let mut seen = std::collections::HashSet::new();
    seen.insert(target.name.as_str());
    let mut cursor = target;
    while let Some(next_name) = cursor.jump.as_deref() {
        let hop = find(topology, next_name).ok_or_else(|| {
            format!(
                "区域「{}」设备「{}」：jump 指向不存在的设备「{next_name}」",
                region_of(topology, cursor.name.as_str()),
                cursor.name
            )
        })?;
        if !seen.insert(hop.name.as_str()) {
            return Err(format!(
                "区域「{}」设备「{}」：jump 循环",
                region_of(topology, target.name.as_str()),
                target.name
            ));
        }
        toward_outer.push(hop);
        cursor = hop;
    }
    toward_outer.reverse();
    Ok(toward_outer)
}

fn jump_spec(chain: &[&Device]) -> Result<Option<String>, String> {
    if chain.is_empty() {
        return Ok(None);
    }
    let mut parts = Vec::with_capacity(chain.len());
    for hop in chain {
        parts.push(format!("{}@{}:{}", hop.user, hop.host, hop.port));
    }
    Ok(Some(parts.join(",")))
}

fn render_jump_config(chain: &[&Device]) -> Result<String, String> {
    let mut body = String::new();
    let directive = identity_directive();
    for (index, hop) in chain.iter().enumerate() {
        let Some(key_path) = hop.identity_file.as_deref() else {
            continue;
        };
        let alias = format!("mt-jump-{index}");
        reject_config_token(&alias, "别名")?;
        reject_config_token(&hop.host, "HostName")?;
        reject_config_token(&hop.user, "User")?;
        if hop.port == 0 {
            return Err("端口无效".to_string());
        }
        // The alias is generated here. The real host is also a pattern so
        // `-J user@host:port` still selects this block.
        push_line(&mut body, &format!("Host {alias} {}", hop.host))?;
        push_line(&mut body, &format!("    HostName {}", hop.host))?;
        push_line(&mut body, &format!("    User {}", hop.user))?;
        push_line(&mut body, &format!("    Port {}", hop.port))?;
        push_line(&mut body, &format!("    {directive} {key_path}"))?;
        body.push('\n');
    }
    if body.is_empty() {
        return Err("跳板配置为空".to_string());
    }
    // `Match all` ends the Host block so Include is not nested inside it.
    push_line(&mut body, "Match all")?;
    push_line(&mut body, "Include ~/.ssh/config")?;
    Ok(body)
}

fn push_line(body: &mut String, line: &str) -> Result<(), String> {
    if line
        .chars()
        .any(|ch| ch == '\n' || ch == '\r' || ch.is_control())
    {
        return Err("ssh 配置含有控制字符".to_string());
    }
    body.push_str(line);
    body.push('\n');
    Ok(())
}

fn reject_config_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') {
        return Err(format!("{label}无效"));
    }
    if value
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '#')
    {
        return Err(format!("{label}不能包含空白、控制字符或 #"));
    }
    Ok(())
}

fn write_temp_config(chain: &[&Device]) -> Result<TempSshConfig, String> {
    let body = render_jump_config(chain)?;
    let (path, mut file) = create_temp_file()?;
    if file
        .write_all(body.as_bytes())
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err("无法写入临时 ssh 配置".to_string());
    }
    drop(file);
    if let Err(err) = restrict_to_current_user(&path) {
        let _ = fs::remove_file(&path);
        return Err(err);
    }
    Ok(TempSshConfig { path, armed: true })
}

fn create_temp_file() -> Result<(PathBuf, std::fs::File), String> {
    for _ in 0..8 {
        let path = temp_config_path();
        match create_private(&path) {
            Ok(file) => return Ok((path, file)),
            Err(_) if path.exists() => continue,
            Err(err) => return Err(err),
        }
    }
    Err("无法创建临时 ssh 配置".to_string())
}

fn temp_config_path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("my-terminal-ssh-{nanos}-{n}.conf"))
}

#[cfg(unix)]
fn create_private(path: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| "无法创建临时 ssh 配置".to_string())
}

#[cfg(windows)]
fn create_private(path: &Path) -> Result<std::fs::File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "无法创建临时 ssh 配置".to_string())
}

#[cfg(unix)]
fn restrict_to_current_user(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "无法限制临时 ssh 配置的访问权限".to_string())
}

/// Best effort: drop inherited ACEs, grant the current user, and remove the
/// well-known Everyone / Authenticated Users / Users SIDs. `icacls` is spawned
/// directly, not through a shell. The caller deletes the file if this fails.
#[cfg(windows)]
fn restrict_to_current_user(path: &Path) -> Result<(), String> {
    let user =
        std::env::var("USERNAME").map_err(|_| "无法限制临时 ssh 配置的访问权限".to_string())?;
    if user.is_empty()
        || user.chars().any(|ch| {
            ch.is_control() || ch.is_whitespace() || matches!(ch, '"' | '%' | '&' | '|' | '<' | '>')
        })
    {
        return Err("无法限制临时 ssh 配置的访问权限".to_string());
    }
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{user}:(F)"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| "无法限制临时 ssh 配置的访问权限".to_string())?;
    if !status.success() {
        return Err("无法限制临时 ssh 配置的访问权限".to_string());
    }
    // Missing SIDs make icacls exit non-zero. That is still a tighter file
    // than the default DACL, so these removals are best-effort.
    let _ = std::process::Command::new("icacls")
        .arg(path)
        .arg("/remove:g")
        .arg("*S-1-1-0")
        .arg("*S-1-5-11")
        .arg("*S-1-5-32-545")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Topology {
        super::super::parse_topology(include_str!("../../tests/fixtures/topology.valid.json"))
            .expect("fixture")
    }

    #[test]
    fn jump_config_is_private_and_removed_on_drop() {
        let topology = sample();
        let plan = plan_device_session_from(&topology, "内网服务器").unwrap();
        let temp = plan.temp_file.expect("jump hop names a key path");
        let path = temp.path().to_path_buf();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("Host mt-jump-0 jump.example.com\n"));
        assert!(text.contains("    HostName jump.example.com\n"));
        assert!(text.contains("    User me\n"));
        assert!(text.contains("    Port 2222\n"));
        assert!(text.contains("~/.ssh/id_example\n"));
        assert!(text.contains("Match all\nInclude ~/.ssh/config\n"));
        assert!(!text.to_lowercase().contains("proxycommand"));
        assert!(text.find("Host ").unwrap() < text.find("Match all").unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        drop(temp);
        assert!(!path.exists());
    }

    #[test]
    fn plain_device_does_not_create_a_config_file() {
        let topology = sample();
        let plan = plan_device_session_from(&topology, "示例路由器").unwrap();
        assert!(plan.temp_file.is_none());
        assert!(!plan.args.iter().any(|arg| arg == "-F"));
    }

    fn plan_device_session_from(_topology: &Topology, name: &str) -> Result<SessionPlan, String> {
        let dir = std::env::temp_dir().join(format!(
            "my-terminal-topology-test-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("topology.json"),
            include_str!("../../tests/fixtures/topology.valid.json"),
        )
        .unwrap();
        let plan = plan_device_session(&dir, name);
        let _ = fs::remove_dir_all(&dir);
        plan
    }

    static NEXT_TEST: AtomicU64 = AtomicU64::new(1);
}

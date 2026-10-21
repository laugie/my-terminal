//! Local sshd: ProxyJump is refused when direct-tcpip is disabled, and a
//! command relay (`nc %h %p`) still reaches the target.
//!
//! Ignored unless `MT_SSHD_IT=1`. Listens on 127.0.0.1 only. Host and user
//! keys are generated here and removed when the test ends.

use std::fs::{self, File};
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use my_terminal_lib::session::resolve_ssh_binary;
use my_terminal_lib::topology::{parse_topology, plan_device_session, ssh_args_for_connection};

#[test]
#[ignore = "local sshd; set MT_SSHD_IT=1"]
fn relay_connects_when_direct_tcpip_is_forbidden() {
    if std::env::var("MT_SSHD_IT").ok().as_deref() != Some("1") {
        return;
    }
    #[cfg(not(unix))]
    panic!("MT_SSHD_IT only runs on unix");
    #[cfg(unix)]
    unix::run();
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;

    struct Lab {
        dir: PathBuf,
        sshds: Vec<Child>,
        known_hosts: PathBuf,
        known_hosts_original: Option<Vec<u8>>,
        created_ssh_dir: bool,
    }

    impl Drop for Lab {
        fn drop(&mut self) {
            for child in &mut self.sshds {
                let pid = child.id();
                let _ = Command::new("kill")
                    .args(["-KILL", &format!("-{pid}")])
                    .output();
                let _ = child.kill();
                let _ = child.wait();
            }
            match &self.known_hosts_original {
                Some(bytes) => {
                    let _ = fs::write(&self.known_hosts, bytes);
                }
                None => {
                    let _ = fs::remove_file(&self.known_hosts);
                }
            }
            if self.created_ssh_dir {
                let dir = self.known_hosts.parent().unwrap_or(Path::new("."));
                let _ = fs::remove_dir(dir);
            }
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    pub fn run() {
        let mut lab = Lab {
            dir: std::env::temp_dir().join(format!(
                "mt-relay-sshd-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0)
            )),
            sshds: Vec::new(),
            known_hosts: PathBuf::new(),
            known_hosts_original: None,
            created_ssh_dir: false,
        };
        fs::create_dir_all(&lab.dir).unwrap();
        let _ = fs::set_permissions(&lab.dir, fs::Permissions::from_mode(0o700));

        let user_key = lab.dir.join("user");
        let jump_host_key = lab.dir.join("jump_host");
        let target_host_key = lab.dir.join("target_host");
        keygen(&user_key);
        keygen(&jump_host_key);
        keygen(&target_host_key);
        fs::copy(
            user_key.with_extension("pub"),
            lab.dir.join("authorized_keys"),
        )
        .unwrap();
        let _ = fs::set_permissions(
            lab.dir.join("authorized_keys"),
            fs::Permissions::from_mode(0o600),
        );

        let jump_port = free_port();
        let target_port = free_port();
        let jump_config = write_sshd(
            &lab.dir.join("jump.conf"),
            jump_port,
            &jump_host_key,
            &lab.dir.join("authorized_keys"),
            &lab.dir.join("jump.pid"),
        );
        let target_config = write_sshd(
            &lab.dir.join("target.conf"),
            target_port,
            &target_host_key,
            &lab.dir.join("authorized_keys"),
            &lab.dir.join("target.pid"),
        );
        lab.sshds
            .push(spawn_sshd(&jump_config, &lab.dir.join("jump.log")));
        lab.sshds
            .push(spawn_sshd(&target_config, &lab.dir.join("target.log")));
        wait_listen(jump_port, &mut lab.sshds[0], &lab.dir.join("jump.log"));
        wait_listen(target_port, &mut lab.sshds[1], &lab.dir.join("target.log"));

        seed_known_hosts(
            &mut lab,
            &[
                (jump_port, &jump_host_key.with_extension("pub")),
                (target_port, &target_host_key.with_extension("pub")),
            ],
        );

        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .expect("USER");
        let key = user_key.to_str().expect("key path").to_string();
        let document = serde_json::json!({
            "regions": [{
                "name": "测试",
                "devices": [
                    {
                        "name": "跳板机",
                        "host": "127.0.0.1",
                        "port": jump_port,
                        "user": user,
                        "identityFile": key,
                    },
                    {
                        "name": "目标机",
                        "host": "127.0.0.1",
                        "port": target_port,
                        "user": user,
                        "identityFile": key,
                        "jump": "跳板机",
                        "connections": [{
                            "name": "nc 中转",
                            "via": "跳板机",
                            "command": "nc %h %p"
                        }]
                    }
                ]
            }]
        });
        let text = serde_json::to_string(&document).unwrap();
        fs::write(lab.dir.join("topology.json"), &text).unwrap();
        let topology = parse_topology(&text).unwrap();
        let relay_args = ssh_args_for_connection(&topology, "目标机", Some("nc 中转")).unwrap();
        assert!(!relay_args.iter().any(|arg| arg == "-J"), "{relay_args:?}");
        assert!(
            relay_args.iter().any(|arg| arg.contains("nc 127.0.0.1 ")),
            "{relay_args:?}"
        );

        let prefix = client_prefix(&lab.known_hosts);
        let jump_plan = plan_device_session(&lab.dir, "目标机").unwrap();
        assert!(
            jump_plan.args.iter().any(|arg| arg == "-J"),
            "{:?}",
            jump_plan.args
        );
        let jump = run_ssh(&prefix, &jump_plan.args, &["echo", "SHOULD-FAIL"]);
        let jump_err = String::from_utf8_lossy(&jump.stderr);
        assert!(
            !jump.status.success(),
            "ProxyJump should fail when direct-tcpip is disabled: {jump_err}"
        );
        assert!(
            jump_err.contains("administratively prohibited"),
            "stderr={jump_err}"
        );
        drop(jump_plan);

        let relay = run_ssh(&prefix, &relay_args, &["echo", "RELAY-OK"]);
        let relay_out = String::from_utf8_lossy(&relay.stdout);
        let relay_err = String::from_utf8_lossy(&relay.stderr);
        assert!(
            relay.status.success(),
            "relay failed\nstdout={relay_out}\nstderr={relay_err}\nargs={relay_args:?}"
        );
        assert!(
            relay_out.contains("RELAY-OK"),
            "stdout={relay_out}\nstderr={relay_err}"
        );
    }

    fn client_prefix(known_hosts: &Path) -> Vec<String> {
        let known = known_hosts.to_str().expect("known_hosts path").to_string();
        vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "IdentitiesOnly=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=yes".to_string(),
            "-o".to_string(),
            format!("UserKnownHostsFile={known}"),
            "-o".to_string(),
            "GlobalKnownHostsFile=/dev/null".to_string(),
            "-o".to_string(),
            "ConnectTimeout=5".to_string(),
        ]
    }

    fn seed_known_hosts(lab: &mut Lab, keys: &[(u16, &Path)]) {
        let home = std::env::var_os("HOME").map(PathBuf::from).expect("HOME");
        let ssh_dir = home.join(".ssh");
        if !ssh_dir.exists() {
            fs::create_dir_all(&ssh_dir).unwrap();
            let _ = fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700));
            lab.created_ssh_dir = true;
        }
        let path = ssh_dir.join("known_hosts");
        lab.known_hosts_original = if path.exists() {
            Some(fs::read(&path).unwrap())
        } else {
            None
        };
        lab.known_hosts = path;
        let mut body = lab.known_hosts_original.clone().unwrap_or_default();
        if !body.is_empty() && !body.ends_with(b"\n") {
            body.push(b'\n');
        }
        for (port, pub_path) in keys {
            let line = format!("[127.0.0.1]:{port} {}\n", pubkey_body(pub_path));
            body.extend_from_slice(line.as_bytes());
        }
        fs::write(&lab.known_hosts, &body).unwrap();
    }

    fn pubkey_body(path: &Path) -> String {
        let text = fs::read_to_string(path).unwrap();
        let mut parts = text.split_whitespace();
        let kind = parts.next().expect("key type");
        let data = parts.next().expect("key data");
        format!("{kind} {data}")
    }

    fn keygen(path: &Path) {
        let status = Command::new("ssh-keygen")
            .args([
                "-q",
                "-t",
                "ed25519",
                "-f",
                path.to_str().unwrap(),
                "-N",
                "",
                "-C",
                "mt-relay",
            ])
            .status()
            .expect("ssh-keygen");
        assert!(status.success(), "ssh-keygen failed for {}", path.display());
    }

    fn write_sshd(
        path: &Path,
        port: u16,
        host_key: &Path,
        authorized: &Path,
        pid: &Path,
    ) -> PathBuf {
        let text = format!(
            "\
Port {port}
ListenAddress 127.0.0.1
AddressFamily inet
HostKey {host_key}
PidFile {pid}
AuthorizedKeysFile {authorized}
PasswordAuthentication no
KbdInteractiveAuthentication no
ChallengeResponseAuthentication no
UsePAM no
PubkeyAuthentication yes
PermitRootLogin no
StrictModes no
AllowTcpForwarding no
PermitOpen none
AllowAgentForwarding no
X11Forwarding no
PermitTTY yes
PrintMotd no
PrintLastLog no
UseDNS no
",
            host_key = host_key.display(),
            authorized = authorized.display(),
            pid = pid.display(),
        );
        fs::write(path, text).unwrap();
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        path.to_path_buf()
    }

    fn spawn_sshd(config: &Path, log_path: &Path) -> Child {
        let sshd = sshd_path();
        let check = Command::new(&sshd)
            .arg("-t")
            .arg("-f")
            .arg(config)
            .output()
            .expect("sshd -t");
        if !check.status.success() {
            panic!(
                "sshd config rejected: {}",
                String::from_utf8_lossy(&check.stderr)
            );
        }
        let log = File::create(log_path).unwrap();
        let mut cmd = Command::new(sshd);
        cmd.arg("-D")
            .arg("-e")
            .arg("-f")
            .arg(config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .process_group(0);
        cmd.spawn().expect("spawn sshd")
    }

    fn sshd_path() -> PathBuf {
        for candidate in ["/usr/sbin/sshd", "/usr/bin/sshd", "/opt/homebrew/sbin/sshd"] {
            if Path::new(candidate).is_file() {
                return PathBuf::from(candidate);
            }
        }
        panic!("未找到 sshd");
    }

    fn wait_listen(port: u16, child: &mut Child, log_path: &Path) {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                let log = fs::read_to_string(log_path).unwrap_or_default();
                panic!("sshd exited {status} before listening on 127.0.0.1:{port}\n{log}");
            }
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                return;
            }
            if Instant::now() >= deadline {
                let log = fs::read_to_string(log_path).unwrap_or_default();
                panic!("sshd did not listen on 127.0.0.1:{port}\n{log}");
            }
            thread::sleep(Duration::from_millis(40));
        }
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    fn run_ssh(prefix: &[String], args: &[String], remote: &[&str]) -> Output {
        let program = resolve_ssh_binary().expect("ssh");
        let mut cmd = Command::new(program);
        cmd.args(prefix)
            .args(args)
            .args(remote)
            .env_remove("SSH_AUTH_SOCK")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn ssh");
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    let mut stdout = Vec::new();
                    let mut stderr = Vec::new();
                    if let Some(mut pipe) = child.stdout.take() {
                        let _ = pipe.read_to_end(&mut stdout);
                    }
                    if let Some(mut pipe) = child.stderr.take() {
                        let _ = pipe.read_to_end(&mut stderr);
                    }
                    let status = child.wait().expect("wait ssh");
                    return Output {
                        status,
                        stdout,
                        stderr,
                    };
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("ssh timed out\nargs={args:?}");
                }
                Ok(None) => thread::sleep(Duration::from_millis(30)),
                Err(err) => panic!("ssh wait failed: {err}"),
            }
        }
    }
}

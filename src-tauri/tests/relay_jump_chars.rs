// Jump-host user and host must not carry `~` or `\` into ProxyCommand.
// A key path may still contain both.
use my_terminal_lib::topology::{parse_topology, ssh_args_for_connection};

fn relay(via_user: &str, via_host: &str, identity: Option<&str>) -> String {
    let key = match identity {
        Some(path) => format!(
            r#","identityFile":{}"#,
            serde_json::to_string(path).unwrap()
        ),
        None => String::new(),
    };
    format!(
        r#"{{"regions":[{{"name":"r","devices":[
            {{"name":"j","host":{host},"user":{user}{key}}},
            {{"name":"t","host":"192.0.2.30","port":2200,"user":"ops","connections":[
                {{"name":"c","via":"j","command":"nc %h %p"}}
            ]}}
        ]}}]}}"#,
        host = serde_json::to_string(via_host).unwrap(),
        user = serde_json::to_string(via_user).unwrap(),
        key = key,
    )
}

fn relay_err(via_user: &str, via_host: &str) -> String {
    let topology = parse_topology(&relay(via_user, via_host, None)).unwrap();
    ssh_args_for_connection(&topology, "t", Some("c")).unwrap_err()
}

#[test]
fn jump_user_with_tilde_is_rejected() {
    let err = relay_err("me~ops", "jump.example.com");
    assert!(err.contains("跳板机参数不能用于中转命令"), "{err}");
}

#[test]
fn jump_user_with_backslash_is_rejected() {
    let err = relay_err(r"me\ops", "jump.example.com");
    assert!(err.contains("跳板机参数不能用于中转命令"), "{err}");
}

#[test]
fn jump_host_with_tilde_is_rejected() {
    let err = relay_err("me", "jump~.example.com");
    assert!(err.contains("跳板机参数不能用于中转命令"), "{err}");
}

#[test]
fn jump_host_with_backslash_is_rejected() {
    let err = relay_err("me", r"jump\.example.com");
    assert!(err.contains("跳板机参数不能用于中转命令"), "{err}");
}

#[test]
fn jump_key_path_may_contain_tilde_and_backslash() {
    let path = r"~\.ssh\id_example";
    let topology = parse_topology(&relay("me", "jump.example.com", Some(path))).unwrap();
    let args = ssh_args_for_connection(&topology, "t", Some("c")).unwrap();
    let proxy = args
        .iter()
        .find(|arg| arg.starts_with("ProxyCommand="))
        .expect("ProxyCommand");
    assert!(proxy.contains(path), "{proxy}");
    assert!(proxy.contains('~') && proxy.contains('\\'), "{proxy}");
}

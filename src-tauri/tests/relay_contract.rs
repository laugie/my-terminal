// #25 验收契约测试：经跳板机执行命令中转（ProxyCommand 式），与原直连/ProxyJump 并存。
// 放到 src-tauri/tests/relay_contract.rs，fixture 放 src-tauri/tests/fixtures/topology.relay.json。
// 期望的公开接口（命名可经 Chief 同意调整，覆盖面不得缩小）：
//   Device.connections: Vec<RelayConnection{ name, via, command }>（缺省为空）
//   ssh_args_for_connection(&Topology, device, Option<&str>) -> Result<Vec<String>, String>
//       None = 默认方式，必须与 ssh_args_for 完全相同
//   expand_relay_template(template, host, port) -> Result<String, String>
//   failure_side(stderr, exit_code) -> "jump" | "target" | "unknown"
// 只用 192.0.2.x / example.com 占位。
use my_terminal_lib::topology::{
    expand_relay_template, failure_side, parse_topology, ssh_args_for, ssh_args_for_connection,
};

const RELAY: &str = include_str!("fixtures/topology.relay.json");
const OLD: &str = include_str!("fixtures/topology.valid.json");

fn with_conn(conn: &str) -> String {
    format!(
        r#"{{"regions":[{{"name":"r","devices":[
        {{"name":"j","host":"jump.example.com","user":"me"}},
        {{"name":"t","host":"192.0.2.30","user":"ops","connections":[{conn}]}}]}}]}}"#
    )
}

#[test]
fn old_config_without_connections_is_unchanged() {
    let t = parse_topology(OLD).unwrap();
    for region in &t.regions {
        for d in &region.devices {
            assert!(d.connections.is_empty(), "{} 不应有额外连接", d.name);
            assert_eq!(
                ssh_args_for_connection(&t, &d.name, None).unwrap(),
                ssh_args_for(&t, &d.name).unwrap(),
                "默认方式必须与原 ssh_args_for 完全一致：{}",
                d.name
            );
        }
    }
}

#[test]
fn parses_connections_and_keeps_default_jump() {
    let t = parse_topology(RELAY).unwrap();
    let d = &t.regions[0].devices[1];
    assert_eq!(d.jump.as_deref(), Some("跳板机"));
    assert_eq!(d.connections.len(), 1);
    assert_eq!(d.connections[0].name, "经跳板命令");
    assert_eq!(d.connections[0].via, "跳板机");
    assert_eq!(d.connections[0].command, "tailscale nc %h %p");
    let default = ssh_args_for_connection(&t, "二级内网", None).unwrap();
    assert!(default.iter().any(|a| a == "-J"), "默认方式仍走 ProxyJump: {default:?}");
}

#[test]
fn template_placeholders_expand() {
    assert_eq!(
        expand_relay_template("tailscale nc %h %p", "192.0.2.30", 2200).unwrap(),
        "tailscale nc 192.0.2.30 2200"
    );
    assert_eq!(expand_relay_template("nc %h %p %%", "app.example.com", 22).unwrap(), "nc app.example.com 22 %");
    assert!(expand_relay_template("nc %h %r", "192.0.2.30", 22).is_err(), "未知占位符应报错");
    assert!(expand_relay_template("nc %p", "192.0.2.30", 22).is_err(), "缺 %h 应报错");
    assert!(expand_relay_template("nc %h", "192.0.2.30", 22).is_err(), "缺 %p 应报错");
}

#[test]
fn template_rejects_shell_metacharacters() {
    for bad in [
        "nc %h %p; id", "nc %h %p | x", "nc %h %p && x", "nc $(x) %h %p", "nc `x` %h %p",
        "nc '%h' %p", "nc \"%h\" %p", "nc %h %p > /tmp/x", "nc %h %p\nid", "nc %h %p \\", "-oX %h %p", "",
    ] {
        assert!(
            parse_topology(&with_conn(&format!(r#"{{"name":"c","via":"j","command":{}}}"#, serde_json::to_string(bad).unwrap()))).is_err(),
            "命令模板应拒绝：{bad:?}"
        );
    }
}

#[test]
fn connection_graph_errors_are_reported() {
    let e = parse_topology(&with_conn(r#"{"name":"c","via":"nope","command":"nc %h %p"}"#)).unwrap_err();
    assert!(e.contains("nope"), "{e}");
    assert!(parse_topology(&with_conn(r#"{"name":"c","via":"t","command":"nc %h %p"}"#)).is_err(), "via 不能是自己");
    assert!(parse_topology(&with_conn(r#"{"name":"c","command":"nc %h %p"}"#)).unwrap_err().contains("via"));
    assert!(parse_topology(&with_conn(r#"{"name":"c","via":"j"}"#)).unwrap_err().contains("command"));
    assert!(parse_topology(&with_conn(r#"{"via":"j","command":"nc %h %p"}"#)).unwrap_err().contains("name"));
    assert!(
        parse_topology(&with_conn(r#"{"name":"c","via":"j","command":"nc %h %p"},{"name":"c","via":"j","command":"nc %h %p"}"#)).is_err(),
        "同一设备下连接名重复"
    );
    assert!(parse_topology(&with_conn(r#"{"name":"默认","via":"j","command":"nc %h %p"}"#)).is_err(), "“默认”为保留名");
}

#[test]
fn relay_args_use_proxycommand_and_no_jump() {
    let t = parse_topology(RELAY).unwrap();
    let args = ssh_args_for_connection(&t, "二级内网", Some("经跳板命令")).unwrap();
    assert!(!args.iter().any(|a| a == "-J"), "中转方式不得再带 -J: {args:?}");
    let dashdash = args.iter().position(|a| a == "--").expect("--");
    assert_eq!(&args[dashdash + 1..], ["192.0.2.30"]);
    let i = args.iter().position(|a| a.starts_with("ProxyCommand=")).expect("ProxyCommand");
    assert!(i > 0 && args[i - 1] == "-o" && i < dashdash);
    let pc = &args[i];
    assert!(
        pc.ends_with(" -T -l me -p 2222 -i ~/.ssh/id_example -- jump.example.com tailscale nc 192.0.2.30 2200"),
        "{pc}"
    );
    assert!(!pc.contains('%'), "占位符应已展开: {pc}");
    for want in ["-l", "ops", "-p", "2200"] {
        assert!(args[..dashdash].iter().any(|a| a == want), "目标机参数缺 {want}: {args:?}");
    }
}

#[test]
fn relay_works_for_device_without_default_jump() {
    let t = parse_topology(RELAY).unwrap();
    let direct = ssh_args_for_connection(&t, "内网路由", None).unwrap();
    assert!(!direct.iter().any(|a| a.starts_with("ProxyCommand=") || a == "-J"));
    let relay = ssh_args_for_connection(&t, "内网路由", Some("nc 中转")).unwrap();
    assert!(relay.iter().any(|a| a.ends_with("-- jump.example.com nc 192.0.2.31 22")), "{relay:?}");
}

#[test]
fn unknown_connection_name_is_an_error() {
    let t = parse_topology(RELAY).unwrap();
    let e = ssh_args_for_connection(&t, "二级内网", Some("不存在")).unwrap_err();
    assert!(e.contains("不存在"), "{e}");
}

#[test]
fn failure_side_tells_jump_from_target() {
    assert_eq!(failure_side("ssh: connect to host jump.example.com port 2222: Connection refused\nConnection closed by UNKNOWN port 65535\n", 255), "jump");
    assert_eq!(failure_side("me@jump.example.com: Permission denied (publickey).\nConnection closed by UNKNOWN port 65535\n", 255), "jump");
    assert_eq!(failure_side("ops@192.0.2.30: Permission denied (publickey).\n", 255), "target");
    assert_eq!(failure_side("", 0), "unknown");
}

// #16 验收契约测试。放到 src-tauri/tests/topology_contract.rs，fixture 放 src-tauri/tests/fixtures/。
// 期望的公开接口（Builder 可在 Chief 同意下调整命名，但覆盖面不得缩小）：
//   my_terminal_lib::topology::{parse_topology, ssh_args_for, Topology, Device}
//   parse_topology(&str) -> Result<Topology, String>     // 错误信息含具体位置/原因
//   ssh_args_for(&Topology, device_name: &str) -> Result<Vec<String>, String>
// 全部只用 192.0.2.x / example.com 占位。
use my_terminal_lib::topology::{parse_topology, ssh_args_for};

const VALID: &str = include_str!("fixtures/topology.valid.json");

fn err(json: &str) -> String {
    parse_topology(json).expect_err("应报错")
}

#[test]
fn parses_regions_and_devices_with_issue_field_names() {
    let t = parse_topology(VALID).unwrap();
    assert_eq!(t.regions.len(), 2);
    assert_eq!(t.regions[0].name, "示例办公室");
    let laptop = &t.regions[0].devices[1];
    assert_eq!(laptop.name, "示例笔记本");
    assert!(laptop.mobile);
    assert_eq!(laptop.tailscale_address.as_deref(), Some("192.0.2.20"));
    let router = &t.regions[0].devices[0];
    assert_eq!(router.port, 22, "port 缺省为 22");
    assert!(!router.mobile && router.identity_file.is_none() && router.jump.is_none());
}

#[test]
fn missing_required_fields_are_named() {
    for (field, json) in [
        ("name", r#"{"regions":[{"name":"r","devices":[{"host":"192.0.2.1","user":"me"}]}]}"#),
        ("host", r#"{"regions":[{"name":"r","devices":[{"name":"a","user":"me"}]}]}"#),
        ("user", r#"{"regions":[{"name":"r","devices":[{"name":"a","host":"192.0.2.1"}]}]}"#),
        ("regions", r#"{}"#),
    ] {
        assert!(err(json).contains(field), "缺 {field} 的报错应点名字段: {}", err(json));
    }
}

#[test]
fn json_syntax_error_reports_line_and_column() {
    let e = err("{\n  \"regions\": [\n");
    assert!(e.contains("行") || e.contains("line"), "{e}");
}

#[test]
fn duplicate_device_names_are_rejected() {
    let e = err(r#"{"regions":[{"name":"r","devices":[
        {"name":"a","host":"192.0.2.1","user":"me"},{"name":"a","host":"192.0.2.2","user":"me"}]}]}"#);
    assert!(e.contains("a") && (e.contains("重复") || e.contains("duplicate")), "{e}");
}

#[test]
fn jump_to_missing_device_is_rejected() {
    let e = err(r#"{"regions":[{"name":"r","devices":[
        {"name":"a","host":"192.0.2.1","user":"me","jump":"不存在"}]}]}"#);
    assert!(e.contains("不存在"), "{e}");
}

#[test]
fn jump_cycles_are_rejected_including_self_and_cross_region() {
    let self_loop = r#"{"regions":[{"name":"r","devices":[{"name":"a","host":"192.0.2.1","user":"me","jump":"a"}]}]}"#;
    let two = r#"{"regions":[{"name":"r1","devices":[{"name":"a","host":"192.0.2.1","user":"me","jump":"b"}]},
                             {"name":"r2","devices":[{"name":"b","host":"192.0.2.2","user":"me","jump":"a"}]}]}"#;
    for j in [self_loop, two] {
        let e = err(j);
        assert!(e.contains("循环") || e.contains("cycle"), "{e}");
    }
}

#[test]
fn port_zero_or_out_of_range_is_rejected() {
    assert!(parse_topology(r#"{"regions":[{"name":"r","devices":[{"name":"a","host":"192.0.2.1","user":"me","port":0}]}]}"#).is_err());
    assert!(parse_topology(r#"{"regions":[{"name":"r","devices":[{"name":"a","host":"192.0.2.1","user":"me","port":70000}]}]}"#).is_err());
}

#[test]
fn plain_device_args() {
    let t = parse_topology(VALID).unwrap();
    assert_eq!(ssh_args_for(&t, "示例路由器").unwrap(),
        vec!["-t", "-l", "admin", "-p", "22", "--", "192.0.2.1"]);
}

#[test]
fn identity_file_is_its_own_argument() {
    let t = parse_topology(VALID).unwrap();
    let a = ssh_args_for(&t, "跳板机").unwrap();
    let i = a.iter().position(|x| x == "-i").expect("-i");
    assert_eq!(a[i + 1], "~/.ssh/id_example");
    assert_eq!(a[a.len() - 2..], ["--".to_string(), "jump.example.com".to_string()]);
}

#[test]
fn jump_carries_jump_host_user_and_port() {
    let t = parse_topology(VALID).unwrap();
    let a = ssh_args_for(&t, "内网服务器").unwrap();
    let j = a.iter().position(|x| x == "-J").expect("-J");
    assert_eq!(a[j + 1], "me@jump.example.com:2222");
    assert_eq!(a.last().unwrap(), "app.example.com");
    assert!(j < a.iter().position(|x| x == "--").unwrap(), "-J 必须在 -- 之前");
}

#[test]
fn chained_jumps_are_ordered_outermost_first() {
    let t = parse_topology(VALID).unwrap();
    let a = ssh_args_for(&t, "二级内网").unwrap();
    let j = a.iter().position(|x| x == "-J").expect("-J");
    assert_eq!(a[j + 1], "me@jump.example.com:2222,me@app.example.com:22");
    assert!(a.windows(2).any(|w| w == ["-p", "2200"]));
}

#[test]
fn injection_in_any_field_is_rejected() {
    let cases = [
        r#"{"name":"x","host":"-oProxyCommand=touch_pwned","user":"me"}"#,
        r#"{"name":"x","host":"192.0.2.1","user":"me;id"}"#,
        r#"{"name":"x","host":"192.0.2.1 -v","user":"me"}"#,
        r#"{"name":"x","host":"192.0.2.1","user":"me","identityFile":"-oProxyCommand=x"}"#,
        r#"{"name":"x","host":"me@192.0.2.1","user":"me"}"#,
        r#"{"name":"x","host":"192.0.2.1,198.51.100.1","user":"me"}"#,
    ];
    for dev in cases {
        let json = format!(r#"{{"regions":[{{"name":"r","devices":[{dev}]}}]}}"#);
        let blocked = match parse_topology(&json) {
            Err(_) => true,
            Ok(t) => ssh_args_for(&t, "x").is_err(),
        };
        assert!(blocked, "必须拒绝: {dev}");
    }
    // 跳板机字段同样校验（-J 值由校验过的 token 拼接，不能注入逗号/空格/选项）
    let json = r#"{"regions":[{"name":"r","devices":[
        {"name":"j","host":"jump.example.com,-oProxyCommand=x","user":"me"},
        {"name":"t","host":"192.0.2.9","user":"me","jump":"j"}]}]}"#;
    let blocked = match parse_topology(json) { Err(_) => true, Ok(t) => ssh_args_for(&t, "t").is_err() };
    assert!(blocked);
}

#[test]
fn unknown_device_name_is_an_error() {
    let t = parse_topology(VALID).unwrap();
    assert!(ssh_args_for(&t, "没有这台").is_err());
}

// #31 失败测试：中转连接可以单独写目标 host / port（例如经 NAS 用 tailscale nc 连目标机的 Tailscale 地址）。
// 不写时仍用设备自己的 host / port。只用 192.0.2.x / example.com 占位。
use my_terminal_lib::topology::{parse_topology, ssh_args_for_connection};

fn with_conn(conn: &str) -> String {
    format!(
        r#"{{"regions":[{{"name":"r","devices":[
        {{"name":"nas","host":"nas.example.com","user":"me"}},
        {{"name":"t","host":"192.0.2.30","port":2200,"user":"ops","connections":[{conn}]}}]}}]}}"#
    )
}

fn split(args: &[String]) -> (&[String], &[String]) {
    let dash = args.iter().position(|a| a == "--").expect("--");
    (&args[..dash], &args[dash + 1..])
}

fn proxy(args: &[String]) -> &str {
    args.iter()
        .find(|a| a.starts_with("ProxyCommand="))
        .expect("ProxyCommand")
}

#[test]
fn connection_host_and_port_replace_target_for_that_route() {
    let t = parse_topology(&with_conn(
        r#"{"name":"经 NAS","via":"nas","command":"tailscale nc %h %p","host":"192.0.2.99","port":2201}"#,
    ))
    .unwrap();
    let c = &t.regions[0].devices[1].connections[0];
    assert_eq!(c.host.as_deref(), Some("192.0.2.99"));
    assert_eq!(c.port, Some(2201));

    let args = ssh_args_for_connection(&t, "t", Some("经 NAS")).unwrap();
    let (opts, target) = split(&args);
    assert_eq!(target, ["192.0.2.99"], "ssh 目标用这条连接的 host: {args:?}");
    assert!(
        proxy(&args).ends_with(" -- nas.example.com tailscale nc 192.0.2.99 2201"),
        "{args:?}"
    );
    let p = opts.iter().position(|a| a == "-p").expect("-p");
    assert_eq!(opts[p + 1], "2201");
    let l = opts.iter().position(|a| a == "-l").expect("-l");
    assert_eq!(opts[l + 1], "ops", "用户名仍是设备的");
    assert!(!args.iter().any(|a| a.contains("192.0.2.30")), "{args:?}");

    let default = ssh_args_for_connection(&t, "t", None).unwrap();
    assert_eq!(split(&default).1, ["192.0.2.30"], "默认方式不受影响");
}

#[test]
fn connection_host_alone_keeps_device_port() {
    let t = parse_topology(&with_conn(
        r#"{"name":"经 NAS","via":"nas","command":"tailscale nc %h %p","host":"t.example.com"}"#,
    ))
    .unwrap();
    assert_eq!(t.regions[0].devices[1].connections[0].port, None);
    let args = ssh_args_for_connection(&t, "t", Some("经 NAS")).unwrap();
    assert_eq!(split(&args).1, ["t.example.com"]);
    assert!(proxy(&args).ends_with(" tailscale nc t.example.com 2200"), "{args:?}");
}

#[test]
fn connection_without_host_or_port_uses_device() {
    let t = parse_topology(&with_conn(
        r#"{"name":"经 NAS","via":"nas","command":"tailscale nc %h %p"}"#,
    ))
    .unwrap();
    let c = &t.regions[0].devices[1].connections[0];
    assert_eq!(c.host, None);
    assert_eq!(c.port, None);
    let args = ssh_args_for_connection(&t, "t", Some("经 NAS")).unwrap();
    assert_eq!(split(&args).1, ["192.0.2.30"]);
    assert!(proxy(&args).ends_with(" tailscale nc 192.0.2.30 2200"), "{args:?}");
}

#[test]
fn bad_connection_host_is_rejected_on_load() {
    for host in [
        "192.0.2.9;id",
        "-oProxyCommand=x",
        "a b",
        "a$b",
        "~/x",
        "a\\\\b",
        "a\\nb",
    ] {
        let conn = format!(
            r#"{{"name":"经 NAS","via":"nas","command":"tailscale nc %h %p","host":"{host}"}}"#
        );
        let err = parse_topology(&with_conn(&conn)).expect_err(host);
        assert!(err.contains("设备「t」"), "{host}: {err}");
    }
    let err = parse_topology(&with_conn(
        r#"{"name":"经 NAS","via":"nas","command":"tailscale nc %h %p","host":7}"#,
    ))
    .unwrap_err();
    assert!(err.contains("host"), "{err}");
}

#[test]
fn bad_connection_port_is_rejected_on_load() {
    for port in ["0", "65536", "\"22\"", "-1", "2.5"] {
        let conn = format!(
            r#"{{"name":"经 NAS","via":"nas","command":"tailscale nc %h %p","port":{port}}}"#
        );
        let err = parse_topology(&with_conn(&conn)).expect_err(port);
        assert!(err.contains("端口"), "{port}: {err}");
    }
}

/// Destination of one system ssh session.
///
/// `user` and `port` are optional. Callers that represent a Tailscale-only
/// row leave `user` empty so the client does not pass `-l`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
}

/// Build argv for the system ssh binary.
///
/// The vector is passed as discrete arguments. It is never joined into a
/// shell command line. Shape: `["-t", ("-l", user)?, ("-p", port)?, "--", host]`.
pub fn build_ssh_args(target: &SshTarget) -> Result<Vec<String>, String> {
    validate_token(&target.host, "主机")?;
    if target.host.contains('@') {
        return Err("主机不能包含 @".to_string());
    }
    if let Some(user) = &target.user {
        validate_token(user, "用户名")?;
        if user.contains('@') {
            return Err("用户名不能包含 @".to_string());
        }
    }
    if let Some(0) = target.port {
        return Err("端口无效".to_string());
    }

    let mut args = Vec::with_capacity(6);
    args.push("-t".to_string());
    if let Some(user) = &target.user {
        args.push("-l".to_string());
        args.push(user.clone());
    }
    if let Some(port) = target.port {
        args.push("-p".to_string());
        args.push(port.to_string());
    }
    args.push("--".to_string());
    args.push(target.host.clone());
    Ok(args)
}

/// One ssh invocation that may name a key path and a jump spec.
///
/// `build_ssh_args` is unchanged. Topology sessions use this constructor so
/// every token still goes through the same checks, plus the extra characters
/// that would change how `-J` or a generated ssh config is parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshOptions {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_path: Option<String>,
    pub jump: Option<String>,
}

pub fn build_ssh_args_with_options(options: &SshOptions) -> Result<Vec<String>, String> {
    validate_endpoint_token(&options.host, "主机")?;
    if let Some(user) = &options.user {
        validate_endpoint_token(user, "用户名")?;
    }
    if let Some(0) = options.port {
        return Err("端口无效".to_string());
    }
    if let Some(path) = &options.identity_path {
        validate_key_path(path)?;
    }
    if let Some(jump) = &options.jump {
        validate_jump_spec(jump)?;
    }

    let mut args = Vec::with_capacity(12);
    args.push("-t".to_string());
    if let Some(user) = &options.user {
        args.push("-l".to_string());
        args.push(user.clone());
    }
    if let Some(port) = options.port {
        args.push("-p".to_string());
        args.push(port.to_string());
    }
    if let Some(path) = &options.identity_path {
        args.push("-i".to_string());
        args.push(path.clone());
    }
    if let Some(jump) = &options.jump {
        args.push("-J".to_string());
        args.push(jump.clone());
    }
    args.push("--".to_string());
    args.push(options.host.clone());
    Ok(args)
}

fn validate_endpoint_token(value: &str, label: &str) -> Result<(), String> {
    validate_token(value, label)?;
    if value.contains('@') || value.contains(',') || value.contains(':') || value.contains('[') {
        return Err(format!("{label}不能包含 @、逗号、冒号或 ["));
    }
    if value.contains('*') || value.contains('?') || value.contains('!') {
        return Err(format!("{label}不能包含通配符"));
    }
    Ok(())
}

fn validate_key_path(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("密钥路径不能为空".to_string());
    }
    if value.starts_with('-') {
        return Err("密钥路径不能以 - 开头".to_string());
    }
    if value
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '#')
    {
        return Err("密钥路径不能包含空白、控制字符或 #".to_string());
    }
    Ok(())
}

fn validate_jump_spec(spec: &str) -> Result<(), String> {
    if spec.is_empty() {
        return Err("跳板参数不能为空".to_string());
    }
    for segment in spec.split(',') {
        let (user, rest) = segment
            .split_once('@')
            .ok_or_else(|| "跳板参数格式无效".to_string())?;
        let (host, port) = rest
            .split_once(':')
            .ok_or_else(|| "跳板参数格式无效".to_string())?;
        validate_endpoint_token(user, "跳板用户名")?;
        validate_endpoint_token(host, "跳板主机")?;
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("跳板端口无效".to_string());
        }
        let port: u32 = port.parse().map_err(|_| "跳板端口无效".to_string())?;
        if port == 0 || port > 65535 {
            return Err("跳板端口无效".to_string());
        }
    }
    Ok(())
}

fn validate_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label}不能为空"));
    }
    if value.starts_with('-') {
        return Err(format!("{label}不能以 - 开头"));
    }
    if value
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(format!("{label}不能包含空格或控制字符"));
    }
    if value.contains(';') {
        return Err(format!("{label}不能包含分号"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(host: &str, user: Option<&str>, port: Option<u16>) -> SshTarget {
        SshTarget {
            host: host.to_string(),
            user: user.map(str::to_string),
            port,
        }
    }

    #[test]
    fn plain_host_has_no_user_or_port_flags() {
        let args = build_ssh_args(&target("host-a.example-tailnet.ts.net", None, None)).unwrap();
        assert_eq!(args, vec!["-t", "--", "host-a.example-tailnet.ts.net"]);
    }

    #[test]
    fn explicit_port_is_its_own_flag() {
        let args = build_ssh_args(&target("100.64.0.1", None, Some(22))).unwrap();
        assert_eq!(args, vec!["-t", "-p", "22", "--", "100.64.0.1"]);
    }

    #[test]
    fn options_put_key_path_and_jump_in_their_own_arguments() {
        let args = build_ssh_args_with_options(&SshOptions {
            host: "192.0.2.1".to_string(),
            user: Some("ops".to_string()),
            port: Some(22),
            identity_path: Some("~/.ssh/id_example".to_string()),
            jump: Some("me@jump.example.com:2222".to_string()),
        })
        .unwrap();
        assert_eq!(
            args,
            vec![
                "-t",
                "-l",
                "ops",
                "-p",
                "22",
                "-i",
                "~/.ssh/id_example",
                "-J",
                "me@jump.example.com:2222",
                "--",
                "192.0.2.1",
            ]
        );
    }

    #[test]
    fn options_reject_a_jump_host_that_smuggles_another_option() {
        let err = build_ssh_args_with_options(&SshOptions {
            host: "192.0.2.9".to_string(),
            user: Some("me".to_string()),
            port: Some(22),
            identity_path: None,
            jump: Some("me@jump.example.com,-oProxyCommand=x:22".to_string()),
        });
        assert!(err.is_err());
    }
}

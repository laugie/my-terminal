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
}

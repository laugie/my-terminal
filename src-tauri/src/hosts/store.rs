/// A server the user added by hand.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualHost {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    pub port: u16,
}

/// Validated fields before the store assigns an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewManualHost {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: u16,
}

/// Storage boundary for manual servers.
pub trait HostStore {
    fn list(&self) -> Vec<ManualHost>;
    fn add(&mut self, host: NewManualHost) -> Result<ManualHost, String>;
    fn remove(&mut self, id: &str) -> Result<(), String>;
}

/// Process-local store. Dropping it drops every manual server.
#[derive(Debug, Default)]
pub struct MemoryHostStore {
    next_id: u64,
    hosts: Vec<ManualHost>,
}

impl MemoryHostStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HostStore for MemoryHostStore {
    fn list(&self) -> Vec<ManualHost> {
        self.hosts.clone()
    }

    fn add(&mut self, host: NewManualHost) -> Result<ManualHost, String> {
        self.next_id += 1;
        let stored = ManualHost {
            id: format!("manual-{}", self.next_id),
            name: host.name,
            host: host.host,
            user: host.user,
            port: host.port,
        };
        self.hosts.push(stored.clone());
        Ok(stored)
    }

    fn remove(&mut self, id: &str) -> Result<(), String> {
        let before = self.hosts.len();
        self.hosts.retain(|host| host.id != id);
        if self.hosts.len() == before {
            Err("未找到该服务器".to_string())
        } else {
            Ok(())
        }
    }
}

const NAME_MAX_CHARS: usize = 64;

pub fn validate_manual_input(
    name: &str,
    host: &str,
    user: Option<&str>,
    port: Option<u32>,
) -> Result<NewManualHost, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if name.chars().count() > NAME_MAX_CHARS {
        return Err("名称不能超过 64 个字符".to_string());
    }

    let host = host.trim();
    if host.is_empty() {
        return Err("主机不能为空".to_string());
    }
    if host.starts_with('-') {
        return Err("主机不能以 - 开头".to_string());
    }
    if host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("主机不能包含空格".to_string());
    }

    let user = match user {
        None => None,
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else if trimmed.starts_with('-') {
                return Err("用户名不能以 - 开头".to_string());
            } else if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err("用户名不能包含空格".to_string());
            } else {
                Some(trimmed.to_string())
            }
        }
    };

    let port = match port {
        None => 22,
        Some(value) if (1..=65535).contains(&value) => value as u16,
        Some(_) => return Err("端口必须在 1 到 65535 之间".to_string()),
    };

    Ok(NewManualHost {
        name: name.to_string(),
        host: host.to_string(),
        user,
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_required_and_limited_to_64_chars() {
        let err = validate_manual_input("   ", "host-a", None, None).unwrap_err();
        assert_eq!(err, "名称不能为空");

        let long = "名".repeat(65);
        let err = validate_manual_input(&long, "host-a", None, None).unwrap_err();
        assert_eq!(err, "名称不能超过 64 个字符");

        let ok = validate_manual_input(&"名".repeat(64), "  host-a  ", None, None).unwrap();
        assert_eq!(ok.name.chars().count(), 64);
        assert_eq!(ok.host, "host-a");
        assert_eq!(ok.port, 22);
        assert_eq!(ok.user, None);
    }

    #[test]
    fn host_rejects_blank_whitespace_and_leading_dash() {
        assert_eq!(
            validate_manual_input("lab", "   ", None, None).unwrap_err(),
            "主机不能为空"
        );
        assert_eq!(
            validate_manual_input("lab", "host a", None, None).unwrap_err(),
            "主机不能包含空格"
        );
        assert_eq!(
            validate_manual_input("lab", "host\ta", None, None).unwrap_err(),
            "主机不能包含空格"
        );
        assert_eq!(
            validate_manual_input("lab", "-host-a", None, None).unwrap_err(),
            "主机不能以 - 开头"
        );
        let ok = validate_manual_input("lab", "host-a", None, Some(22)).unwrap();
        assert_eq!(ok.host, "host-a");
    }

    #[test]
    fn port_defaults_to_22_and_rejects_out_of_range() {
        assert_eq!(
            validate_manual_input("lab", "host-a", None, None)
                .unwrap()
                .port,
            22
        );
        assert_eq!(
            validate_manual_input("lab", "host-a", None, Some(1))
                .unwrap()
                .port,
            1
        );
        assert_eq!(
            validate_manual_input("lab", "host-a", None, Some(65535))
                .unwrap()
                .port,
            65535
        );
        assert_eq!(
            validate_manual_input("lab", "host-a", None, Some(0)).unwrap_err(),
            "端口必须在 1 到 65535 之间"
        );
        assert_eq!(
            validate_manual_input("lab", "host-a", None, Some(65536)).unwrap_err(),
            "端口必须在 1 到 65535 之间"
        );
    }

    #[test]
    fn user_is_optional_and_rejects_unsafe_values() {
        let omitted = validate_manual_input("lab", "host-a", None, None).unwrap();
        assert_eq!(omitted.user, None);
        let blank = validate_manual_input("lab", "host-a", Some("  "), None).unwrap();
        assert_eq!(blank.user, None);
        let kept = validate_manual_input("lab", "host-a", Some(" user "), None).unwrap();
        assert_eq!(kept.user.as_deref(), Some("user"));
        assert_eq!(
            validate_manual_input("lab", "host-a", Some("us er"), None).unwrap_err(),
            "用户名不能包含空格"
        );
        assert_eq!(
            validate_manual_input("lab", "host-a", Some("-user"), None).unwrap_err(),
            "用户名不能以 - 开头"
        );
    }

    #[test]
    fn memory_store_adds_and_removes_without_sharing_state() {
        let mut store = MemoryHostStore::new();
        let store_ref: &mut dyn HostStore = &mut store;
        let added = store_ref
            .add(validate_manual_input("lab", "host-a", Some("user"), Some(2222)).unwrap())
            .unwrap();
        assert_eq!(added.id, "manual-1");
        assert_eq!(added.port, 2222);
        assert_eq!(store_ref.list().len(), 1);

        let second = store_ref
            .add(validate_manual_input("other", "host-b", None, None).unwrap())
            .unwrap();
        assert_eq!(second.id, "manual-2");
        store_ref.remove("manual-1").unwrap();
        let left = store_ref.list();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "manual-2");
        assert_eq!(store_ref.remove("manual-1").unwrap_err(), "未找到该服务器");

        let fresh = MemoryHostStore::new();
        assert!(fresh.list().is_empty());
    }
}

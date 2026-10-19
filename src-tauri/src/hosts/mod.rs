mod exec;
mod list;
mod parse;
mod store;

use std::sync::Mutex;

pub use exec::tailscale_status;
pub use list::{compose_list, HostEntry, ListHostsResponse};
pub use parse::{parse_status, ParseError, TailnetStatus};
pub use store::{HostStore, ManualHost, MemoryHostStore};

use list::compose_list as build_list;
use store::{validate_manual_input, NewManualHost};

/// Process-local manual servers. Nothing here is written to disk.
pub struct HostState {
    store: Mutex<MemoryHostStore>,
}

impl Default for HostState {
    fn default() -> Self {
        Self {
            store: Mutex::new(MemoryHostStore::new()),
        }
    }
}

impl HostState {
    fn with_store<T>(&self, f: impl FnOnce(&mut MemoryHostStore) -> T) -> Result<T, String> {
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "内部状态不可用".to_string())?;
        Ok(f(&mut guard))
    }
}

/// Read Tailscale once and merge the result with manual servers.
///
/// Manual servers are cloned before the CLI runs, so the store lock is not
/// held across the await. The process itself runs on a blocking thread.
#[tauri::command]
pub async fn list_hosts(state: tauri::State<'_, HostState>) -> Result<ListHostsResponse, String> {
    let manuals = state.with_store(|store| store.list())?;
    let output = tauri::async_runtime::spawn_blocking(tailscale_status)
        .await
        .map_err(|_| "读取 Tailscale 状态失败".to_string())?;
    Ok(build_list(&output, &manuals))
}

/// Validate and remember a manual server until the process exits.
#[tauri::command]
pub fn add_manual_host(
    state: tauri::State<'_, HostState>,
    name: String,
    host: String,
    user: Option<String>,
    port: Option<u32>,
) -> Result<ManualHost, String> {
    let validated: NewManualHost = validate_manual_input(&name, &host, user.as_deref(), port)?;
    state.with_store(|store| store.add(validated))
}

/// Drop a manual server by the id returned when it was added.
#[tauri::command]
pub fn remove_manual_host(state: tauri::State<'_, HostState>, id: String) -> Result<(), String> {
    state.with_store(|store| store.remove(&id))?
}

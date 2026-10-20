//! Credential store. Production uses one keyring entry. Tests use memory.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// `keyring` service name. One entry holds the whole JSON record.
pub const KEYRING_SERVICE: &str = "my-terminal";

/// Entry account name. This is not a username and not a secret.
const KEYRING_ACCOUNT: &str = "auth";

/// The store could not be read or written. Callers must fail closed.
#[derive(Debug)]
pub struct StoreError;

/// Object-safe credential store. The running app uses [`KeyringStore`].
pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, StoreError>;
    fn save(&self, value: &str) -> Result<(), StoreError>;
}

/// JSON saved in the single keyring entry. The secret itself is never stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Record {
    pub username: String,
    pub phc: String,
    pub failures: u32,
    pub not_before_ms: i64,
    pub last_seen_ms: i64,
}

/// In-memory store for tests. It never touches the real keychain.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<MemoryState>,
}

#[derive(Debug, Default)]
struct MemoryState {
    value: Option<String>,
    unavailable: bool,
}

impl MemoryStore {
    pub fn dump_all(&self) -> String {
        let guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        guard.value.clone().unwrap_or_default()
    }

    pub fn hash_entry(&self) -> String {
        let raw = self.dump_all();
        serde_json::from_str::<Record>(&raw)
            .map(|record| record.phc)
            .unwrap_or_default()
    }

    pub fn set_unavailable(&self, unavailable: bool) {
        let mut guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        guard.unavailable = unavailable;
    }
}

impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        let guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        if guard.unavailable {
            return Err(StoreError);
        }
        Ok(guard.value.clone())
    }

    fn save(&self, value: &str) -> Result<(), StoreError> {
        let mut guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        if guard.unavailable {
            return Err(StoreError);
        }
        guard.value = Some(value.to_string());
        Ok(())
    }
}

/// One JSON blob in the system keychain (macOS) or credential manager (Windows).
pub struct KeyringStore {
    entry: Option<keyring::Entry>,
}

impl KeyringStore {
    pub fn open() -> Self {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).ok();
        Self { entry }
    }
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        let Some(entry) = &self.entry else {
            return Err(StoreError);
        };
        match read_blob(entry) {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(StoreError),
        }
    }

    fn save(&self, value: &str) -> Result<(), StoreError> {
        let Some(entry) = &self.entry else {
            return Err(StoreError);
        };
        write_blob(entry, value).map_err(|_| StoreError)
    }
}

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/keyring_impl.rs"));

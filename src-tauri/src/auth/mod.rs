//! Startup login lock.
//!
//! The hash and the failure counter live in one system-credential entry.
//! Unlocking is remembered only in this process: a restart asks again.

mod hash;
mod store;
mod throttle;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use hash::{hash_secret, verify_secret, HashParams};
use serde::Serialize;
use store::Record;
use tauri::Manager;
use throttle::{delay_for_failures, duration_ms, observe, remaining, Deadline};

pub use store::{KeyringStore, MemoryStore, SecretStore, StoreError};
pub use throttle::{Clock, FakeClock, SystemClock};

const USERNAME_MAX_CHARS: usize = 64;
const SECRET_MIN_CHARS: usize = 8;

/// What the login screen should show. `Unlocked` is only true in this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    NeedsSetup,
    Locked,
    Unlocked,
    Unavailable,
}

/// Why a setup or login attempt was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginError {
    BadCredentials,
    Throttled { remaining: Duration },
    Unavailable,
    InvalidInput { message: &'static str },
    AlreadySet,
    Locked,
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginError::BadCredentials => f.write_str("用户名或密码错误"),
            LoginError::Throttled { remaining } => {
                let seconds = remaining.as_secs().max(1);
                write!(f, "请等待 {seconds} 秒")
            }
            LoginError::Unavailable => f.write_str("无法读取系统凭据库"),
            LoginError::InvalidInput { message } => f.write_str(message),
            LoginError::AlreadySet => f.write_str("已经设置过，请直接登录"),
            LoginError::Locked => f.write_str("需要登录"),
        }
    }
}

/// Public view of the lock. It never includes the hash or the secret.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSnapshot {
    pub state: String,
    pub retry_after_ms: u64,
    pub message: Option<String>,
}

/// Login lock over a [`SecretStore`] and a [`Clock`].
pub struct AuthService<S = MemoryStore, C = FakeClock> {
    store: Arc<S>,
    clock: Arc<C>,
    unlocked: Mutex<bool>,
    params: HashParams,
}

impl<S, C> AuthService<S, C>
where
    S: SecretStore,
    C: Clock,
{
    /// Constructor used by the contract tests. Hashing uses the fast parameters.
    pub fn new(store: Arc<S>, clock: Arc<C>) -> Self {
        Self::with_params(store, clock, HashParams::fast())
    }

    /// Constructor used by the running app. Hashing uses the OWASP parameters.
    pub fn production(store: Arc<S>, clock: Arc<C>) -> Self {
        Self::with_params(store, clock, HashParams::production())
    }

    fn with_params(store: Arc<S>, clock: Arc<C>, params: HashParams) -> Self {
        Self {
            store,
            clock,
            unlocked: Mutex::new(false),
            params,
        }
    }

    pub fn status(&self) -> AuthStatus {
        match self.read_record() {
            Err(_) => AuthStatus::Unavailable,
            Ok(None) => AuthStatus::NeedsSetup,
            Ok(Some(_)) => {
                if self.is_unlocked() {
                    AuthStatus::Unlocked
                } else {
                    AuthStatus::Locked
                }
            }
        }
    }

    pub fn snapshot(&self) -> AuthSnapshot {
        let status = self.status();
        let retry_after_ms = self.retry_after().as_millis() as u64;
        let message = match status {
            AuthStatus::Unavailable => Some("无法读取系统凭据库".to_string()),
            _ => None,
        };
        AuthSnapshot {
            state: status_name(status).to_string(),
            retry_after_ms,
            message,
        }
    }

    pub fn retry_after(&self) -> Duration {
        let Ok(Some(record)) = self.read_record() else {
            return Duration::ZERO;
        };
        let deadline = Deadline {
            not_before_ms: record.not_before_ms,
            last_seen_ms: record.last_seen_ms,
        };
        remaining(&deadline, self.clock.now_ms())
    }

    /// Forget the in-memory unlock. The stored hash and failure counter stay.
    pub fn lock(&self) {
        self.set_unlocked(false);
    }

    pub fn setup(&self, username: &str, secret: &str, confirm: &str) -> Result<(), LoginError> {
        validate_new_credentials(username, secret, confirm)?;
        if self.read_record()?.is_some() {
            return Err(LoginError::AlreadySet);
        }
        let phc = hash_secret(self.params, secret).map_err(|_| LoginError::Unavailable)?;
        let now = self.clock.now_ms();
        let record = Record {
            username: username.to_string(),
            phc,
            failures: 0,
            not_before_ms: 0,
            last_seen_ms: now,
        };
        self.write_record(&record)?;
        self.set_unlocked(true);
        Ok(())
    }

    pub fn login(&self, username: &str, secret: &str) -> Result<(), LoginError> {
        let Some(mut record) = self.read_record()? else {
            return Err(LoginError::BadCredentials);
        };
        let now = self.clock.now_ms();
        let mut deadline = Deadline {
            not_before_ms: record.not_before_ms,
            last_seen_ms: record.last_seen_ms,
        };
        observe(&mut deadline, now);
        let wait = remaining(&deadline, now);
        if !wait.is_zero() {
            record.not_before_ms = deadline.not_before_ms;
            record.last_seen_ms = deadline.last_seen_ms;
            self.write_record(&record)?;
            return Err(LoginError::Throttled { remaining: wait });
        }
        // Always run the hash check so a wrong username is not a faster answer.
        let secret_ok = verify_secret(&record.phc, secret);
        let username_ok = record.username == username;
        if !(username_ok && secret_ok) {
            record.failures = record.failures.saturating_add(1);
            record.last_seen_ms = now;
            if let Some(delay) = delay_for_failures(record.failures) {
                record.not_before_ms = now.saturating_add(duration_ms(delay));
            } else {
                record.not_before_ms = deadline.not_before_ms;
            }
            self.write_record(&record)?;
            return Err(LoginError::BadCredentials);
        }
        record.failures = 0;
        record.not_before_ms = 0;
        record.last_seen_ms = now;
        self.write_record(&record)?;
        self.set_unlocked(true);
        Ok(())
    }

    fn is_unlocked(&self) -> bool {
        match self.unlocked.lock() {
            Ok(guard) => *guard,
            Err(_) => false,
        }
    }

    fn set_unlocked(&self, unlocked: bool) {
        match self.unlocked.lock() {
            Ok(mut guard) => *guard = unlocked,
            Err(poisoned) => *poisoned.into_inner() = unlocked,
        }
    }

    fn read_record(&self) -> Result<Option<Record>, LoginError> {
        match self.store.load() {
            Ok(None) => Ok(None),
            Ok(Some(raw)) => match serde_json::from_str(&raw) {
                Ok(record) => Ok(Some(record)),
                Err(_) => Err(LoginError::Unavailable),
            },
            Err(StoreError) => Err(LoginError::Unavailable),
        }
    }

    fn write_record(&self, record: &Record) -> Result<(), LoginError> {
        let raw = serde_json::to_string(record).map_err(|_| LoginError::Unavailable)?;
        self.store.save(&raw).map_err(|_| LoginError::Unavailable)
    }
}

/// Block host and session commands until this process has logged in.
pub fn require_unlocked<S, C>(service: &AuthService<S, C>) -> Result<(), LoginError>
where
    S: SecretStore,
    C: Clock,
{
    if service.is_unlocked() {
        Ok(())
    } else {
        Err(LoginError::Locked)
    }
}

fn validate_new_credentials(username: &str, secret: &str, confirm: &str) -> Result<(), LoginError> {
    let username_len = username.chars().count();
    if username_len == 0 || username_len > USERNAME_MAX_CHARS {
        return Err(LoginError::InvalidInput {
            message: "用户名需要 1 到 64 个字符",
        });
    }
    if secret != confirm {
        return Err(LoginError::InvalidInput {
            message: "两次输入的密码不一致",
        });
    }
    if secret.chars().count() < SECRET_MIN_CHARS {
        return Err(LoginError::InvalidInput {
            message: "密码至少 8 个字符",
        });
    }
    Ok(())
}

fn status_name(status: AuthStatus) -> &'static str {
    match status {
        AuthStatus::NeedsSetup => "needsSetup",
        AuthStatus::Locked => "locked",
        AuthStatus::Unlocked => "unlocked",
        AuthStatus::Unavailable => "unavailable",
    }
}

/// Process-wide lock. Commands receive this as Tauri state.
pub type AuthState = AuthService<KeyringStore, SystemClock>;

pub fn open_state() -> AuthState {
    AuthService::production(Arc::new(KeyringStore::open()), Arc::new(SystemClock))
}

/// Read the lock without returning the hash.
#[tauri::command]
pub async fn auth_status(app: tauri::AppHandle) -> Result<AuthSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AuthState>();
        Ok(state.snapshot())
    })
    .await
    .map_err(|_| "无法读取系统凭据库".to_string())?
}

/// First-run setup. A matching confirm and a long enough secret are required.
///
/// Argon2 and the credential write run on a blocking thread. The service lock
/// is not held across `.await`.
#[tauri::command]
pub async fn auth_setup(
    app: tauri::AppHandle,
    username: String,
    secret: String,
    confirm: String,
) -> Result<AuthSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AuthState>();
        state
            .setup(&username, &secret, &confirm)
            .map_err(|err| err.to_string())?;
        Ok(state.snapshot())
    })
    .await
    .map_err(|_| "无法保存凭据".to_string())?
}

/// Check the username and secret. Wrong values share one error string.
///
/// Argon2 and the credential read run on a blocking thread. The service lock
/// is not held across `.await`.
#[tauri::command]
pub async fn auth_login(
    app: tauri::AppHandle,
    username: String,
    secret: String,
) -> Result<AuthSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AuthState>();
        state
            .login(&username, &secret)
            .map_err(|err| err.to_string())?;
        Ok(state.snapshot())
    })
    .await
    .map_err(|_| "登录失败".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        format!("{tag}-{nanos:x}-{:x}", std::process::id())
    }

    fn fresh() -> AuthService<MemoryStore, FakeClock> {
        AuthService::new(
            Arc::new(MemoryStore::default()),
            Arc::new(FakeClock::default()),
        )
    }

    #[test]
    fn production_constructor_keeps_the_owasp_parameters() {
        let service = AuthService::production(
            Arc::new(MemoryStore::default()),
            Arc::new(FakeClock::default()),
        );
        assert_eq!(service.params, HashParams::production());
        assert_eq!(service.params.memory_kib, 19 * 1024);
        assert_eq!(service.params.time_cost, 2);
        assert_eq!(service.params.parallelism, 1);
    }

    #[test]
    fn username_and_secret_lengths_follow_the_limits() {
        let service = fresh();
        let secret = token("p");
        let mut long_name = token("u");
        while long_name.chars().count() < 65 {
            long_name.push('x');
        }
        assert!(service.setup(&long_name, &secret, &secret).is_err());
        let name64: String = long_name.chars().take(64).collect();
        let eight: String = secret.chars().take(8).collect();
        service.setup(&name64, &eight, &eight).expect("64 and 8");
        let other = fresh();
        let seven: String = secret.chars().take(7).collect();
        assert!(other.setup(&token("u"), &seven, &seven).is_err());
    }

    #[test]
    fn rollback_does_not_change_the_remaining_wait() {
        let clock = Arc::new(FakeClock::default());
        let service = AuthService::new(Arc::new(MemoryStore::default()), Arc::clone(&clock));
        let username = token("u");
        let secret = token("p");
        service.setup(&username, &secret, &secret).unwrap();
        service.lock();
        for _ in 0..5 {
            let _ = service.login(&username, &token("no"));
        }
        let before = match service.login(&username, &secret) {
            Err(LoginError::Throttled { remaining }) => remaining,
            other => panic!("expected throttle, got {other:?}"),
        };
        clock.rewind(Duration::from_secs(3600));
        let after = match service.login(&username, &secret) {
            Err(LoginError::Throttled { remaining }) => remaining,
            other => panic!("expected throttle, got {other:?}"),
        };
        assert_eq!(before, after);
        assert!(after > Duration::from_secs(29));
    }
}

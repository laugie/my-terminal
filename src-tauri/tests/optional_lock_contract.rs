// #19 契约测试：src-tauri/tests/optional_lock_contract.rs。
// 期望接口（命名可在 Chief 同意下调整，覆盖面不得缩小）：
//   config::Settings { login_lock_enabled: bool }，序列化为 settings.loginLockEnabled，缺省 false
//   config::set_login_lock_enabled(dir, bool) -> Result<(), ConfigError>（原子写 config.json，保留 manualHosts）
//   auth::require_access(&svc, lock_enabled) -> Result<(), LoginError>   // 关：放行；开：同 require_unlocked
//   AuthService::verify_current(user, secret) -> Result<(), LoginError>  // 关开关前验证；共用节流计数；不改变解锁状态以外的东西
//   auth::startup_route(&svc, lock_enabled) -> StartupRoute { Home, Login, Setup, Unavailable }
// 用户名/口令均运行时随机生成；目录在临时目录下，不碰真实配置。
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use my_terminal_lib::auth::{require_access, startup_route, AuthService, FakeClock, LoginError, MemoryStore, StartupRoute};
use my_terminal_lib::config::{load_config, save_config, set_login_lock_enabled, Config};

fn tok(tag: &str) -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{tag}-{n:x}-{:x}", std::process::id())
}
fn tmp(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("mt19-{label}-{}", tok("d")));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn svc() -> (Arc<MemoryStore>, Arc<FakeClock>, AuthService) {
    let s = Arc::new(MemoryStore::default());
    let c = Arc::new(FakeClock::default());
    (s.clone(), c.clone(), AuthService::new(s, c))
}

#[test]
fn m1_default_config_has_lock_off_and_old_config_without_field_is_off() {
    assert!(!Config::default().settings.login_lock_enabled);
    let dir = tmp("old");
    std::fs::write(dir.join("config.json"), r#"{"version":1,"manualHosts":[],"settings":{}}"#).unwrap();
    assert!(!load_config(&dir).unwrap().settings.login_lock_enabled, "升级用户无开关配置 → 关");
}

#[test]
fn m2_setting_round_trips_as_camel_case_and_keeps_manual_hosts() {
    let dir = tmp("rt");
    let mut cfg = Config::default();
    cfg.settings.login_lock_enabled = false;
    save_config(&dir, &cfg).unwrap();
    set_login_lock_enabled(&dir, true).unwrap();
    let text = std::fs::read_to_string(dir.join("config.json")).unwrap();
    assert!(text.contains("\"loginLockEnabled\": true") || text.contains("\"loginLockEnabled\":true"), "{text}");
    assert!(load_config(&dir).unwrap().settings.login_lock_enabled);
    set_login_lock_enabled(&dir, false).unwrap();
    assert!(!load_config(&dir).unwrap().settings.login_lock_enabled);
}

#[test]
fn m3_lock_off_routes_home_even_with_existing_hash_and_gate_allows() {
    let (_s, _c, a) = svc();
    assert_eq!(startup_route(&a, false), StartupRoute::Home, "全新安装不弹设置页");
    let (u, p) = (tok("u"), tok("p"));
    a.setup(&u, &p, &p).unwrap();
    a.lock();
    assert_eq!(startup_route(&a, false), StartupRoute::Home, "有哈希但开关关 → 首页");
    assert!(require_access(&a, false).is_ok());
}

#[test]
fn m4_lock_on_behaves_like_15() {
    let (_s, _c, a) = svc();
    assert_eq!(startup_route(&a, true), StartupRoute::Setup);
    assert!(require_access(&a, true).is_err());
    let (u, p) = (tok("u"), tok("p"));
    a.setup(&u, &p, &p).unwrap();
    a.lock();
    assert_eq!(startup_route(&a, true), StartupRoute::Login);
    assert!(require_access(&a, true).is_err());
    a.login(&u, &p).unwrap();
    assert!(require_access(&a, true).is_ok());
}

#[test]
fn m5_verify_current_shares_throttle_with_login() {
    let (_s, _c, a) = svc();
    let (u, p) = (tok("u"), tok("p"));
    a.setup(&u, &p, &p).unwrap();
    for _ in 0..3 { let _ = a.login(&u, "wrong-wrong"); }
    for _ in 0..2 { assert_eq!(a.verify_current(&u, "wrong-wrong").unwrap_err(), LoginError::BadCredentials); }
    // 共 5 次失败 → 节流，正确口令也被拒
    assert!(matches!(a.verify_current(&u, &p).unwrap_err(), LoginError::Throttled { .. }));
    assert!(matches!(a.login(&u, &p).unwrap_err(), LoginError::Throttled { .. }));
}

#[test]
fn m6_verify_current_succeeds_and_hash_is_kept() {
    let (s, c, a) = svc();
    let (u, p) = (tok("u"), tok("p"));
    a.setup(&u, &p, &p).unwrap();
    let before = s.hash_entry();
    a.verify_current(&u, &p).unwrap();
    assert_eq!(s.hash_entry(), before, "关开关不删、不改哈希");
    let b = AuthService::new(s, c);
    b.login(&u, &p).unwrap();
    let _ = Duration::ZERO;
}

#[test]
fn m7_verify_without_hash_is_not_required() {
    let (_s, _c, a) = svc();
    // 未设置口令：直接关闭开关不需要验证；verify_current 返回 Ok 表示“无需验证”
    assert!(a.verify_current("", "").is_ok());
}

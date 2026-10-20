// #15 登录锁验收契约测试。放到 src-tauri/tests/auth_contract.rs。
// 期望的公开接口（命名可在 Chief 同意下调整，覆盖面不得缩小）：
//   my_terminal_lib::auth::{AuthService, MemoryStore, SecretStore, Clock, LoginError, AuthStatus}
//   SecretStore：凭据库抽象（生产用 keyring crate；测试用 MemoryStore，不碰真实钥匙串）
//   Clock：可注入时间（测试用 FakeClock，不 sleep）
// 测试里的用户名/口令都在运行时随机生成，仓库里不出现固定口令。
use std::sync::Arc;
use std::time::Duration;

use my_terminal_lib::auth::{AuthService, AuthStatus, FakeClock, LoginError, MemoryStore, SecretStore};

fn rand_token(tag: &str) -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{tag}-{n:x}-{:x}", std::process::id())
}

fn fresh() -> (Arc<MemoryStore>, Arc<FakeClock>, AuthService) {
    let store = Arc::new(MemoryStore::default());
    let clock = Arc::new(FakeClock::default());
    let svc = AuthService::new(store.clone(), clock.clone());
    (store, clock, svc)
}

#[test]
fn first_run_needs_setup_then_login() {
    let (store, clock, svc) = fresh();
    assert_eq!(svc.status(), AuthStatus::NeedsSetup);
    let (u, p) = (rand_token("u"), rand_token("p"));
    svc.setup(&u, &p, &p).unwrap();
    assert_eq!(svc.status(), AuthStatus::Unlocked, "设置完成直接进入主界面");
    // 模拟重启
    let svc2 = AuthService::new(store, clock);
    assert_eq!(svc2.status(), AuthStatus::Locked);
    svc2.login(&u, &p).unwrap();
    assert_eq!(svc2.status(), AuthStatus::Unlocked);
}

#[test]
fn setup_rejects_mismatch_empty_and_second_setup() {
    let (_s, _c, svc) = fresh();
    let p = rand_token("p");
    assert!(svc.setup(&rand_token("u"), &p, &rand_token("other")).is_err());
    assert!(svc.setup("", &p, &p).is_err());
    assert!(svc.setup(&rand_token("u"), "", "").is_err());
    assert!(svc.setup(&rand_token("u"), "short", "short").is_err(), "口令至少 8 位（建议值）");
    svc.setup(&rand_token("u"), &p, &p).unwrap();
    assert!(svc.setup(&rand_token("u"), &p, &p).is_err(), "已设置后不能再次 setup 覆盖");
}

#[test]
fn store_holds_only_argon2id_hash_never_plaintext() {
    let (store, _c, svc) = fresh();
    let (u, p) = (rand_token("u"), rand_token("p"));
    svc.setup(&u, &p, &p).unwrap();
    let dump = store.dump_all();
    assert!(dump.contains("$argon2id$"), "应存 argon2id PHC 串");
    assert!(!dump.contains(&p), "不得存明文口令");
    // 随机盐：同一口令两次设置得到不同哈希
    let (store2, _c2, svc2) = fresh();
    svc2.setup(&u, &p, &p).unwrap();
    assert_ne!(store.hash_entry(), store2.hash_entry());
}

#[test]
fn wrong_user_or_password_give_identical_error() {
    let (_s, _c, svc) = fresh();
    let (u, p) = (rand_token("u"), rand_token("p"));
    svc.setup(&u, &p, &p).unwrap();
    svc.lock();
    let e1 = svc.login(&rand_token("x"), &p).unwrap_err();
    let e2 = svc.login(&u, &rand_token("x")).unwrap_err();
    assert_eq!(e1, LoginError::BadCredentials);
    assert_eq!(e1.to_string(), "用户名或密码错误");
    assert_eq!(e1.to_string(), e2.to_string());
    assert_eq!(svc.status(), AuthStatus::Locked);
}

#[test]
fn five_failures_trigger_30s_then_increasing_delay() {
    let (_s, clock, svc) = fresh();
    let (u, p) = (rand_token("u"), rand_token("p"));
    svc.setup(&u, &p, &p).unwrap();
    svc.lock();
    for _ in 0..4 {
        assert_eq!(svc.login(&u, "wrong-wrong").unwrap_err(), LoginError::BadCredentials);
    }
    assert_eq!(svc.login(&u, "wrong-wrong").unwrap_err(), LoginError::BadCredentials);
    // 第 5 次失败后进入 30s 等待，期间正确口令也拒绝
    match svc.login(&u, &p).unwrap_err() {
        LoginError::Throttled { remaining } => assert!(remaining <= Duration::from_secs(30) && remaining > Duration::from_secs(29)),
        e => panic!("应为 Throttled, got {e:?}"),
    }
    clock.advance(Duration::from_secs(30));
    assert_eq!(svc.login(&u, "wrong-wrong").unwrap_err(), LoginError::BadCredentials);
    match svc.login(&u, &p).unwrap_err() {
        LoginError::Throttled { remaining } => assert!(remaining > Duration::from_secs(30), "再错递增（建议翻倍到 60s）"),
        e => panic!("{e:?}"),
    }
    clock.advance(Duration::from_secs(60));
    svc.login(&u, &p).unwrap();
    // 成功后计数清零
    svc.lock();
    for _ in 0..4 { let _ = svc.login(&u, "wrong-wrong"); }
    svc.login(&u, &p).unwrap();
}

#[test]
fn delay_is_capped() {
    let (_s, clock, svc) = fresh();
    let (u, p) = (rand_token("u"), rand_token("p"));
    svc.setup(&u, &p, &p).unwrap();
    svc.lock();
    for _ in 0..30 {
        let _ = svc.login(&u, "wrong-wrong");
        clock.advance(Duration::from_secs(3600));
    }
    let _ = svc.login(&u, "wrong-wrong");
    if let Err(LoginError::Throttled { remaining }) = svc.login(&u, &p) {
        assert!(remaining <= Duration::from_secs(15 * 60), "上限 15 分钟（建议值）");
    }
}

#[test]
fn throttle_survives_restart() {
    let (store, clock, svc) = fresh();
    let (u, p) = (rand_token("u"), rand_token("p"));
    svc.setup(&u, &p, &p).unwrap();
    svc.lock();
    for _ in 0..5 { let _ = svc.login(&u, "wrong-wrong"); }
    drop(svc);
    let svc2 = AuthService::new(store, clock);
    assert!(matches!(svc2.login(&u, &p).unwrap_err(), LoginError::Throttled { .. }), "重启不能清零");
}

#[test]
fn clock_rollback_does_not_shorten_wait() {
    let (_s, clock, svc) = fresh();
    let (u, p) = (rand_token("u"), rand_token("p"));
    svc.setup(&u, &p, &p).unwrap();
    svc.lock();
    for _ in 0..5 { let _ = svc.login(&u, "wrong-wrong"); }
    clock.rewind(Duration::from_secs(3600));
    assert!(matches!(svc.login(&u, &p).unwrap_err(), LoginError::Throttled { .. }));
}

#[test]
fn store_failure_fails_closed() {
    let (store, clock, svc) = fresh();
    let (u, p) = (rand_token("u"), rand_token("p"));
    svc.setup(&u, &p, &p).unwrap();
    store.set_unavailable(true);
    let svc2 = AuthService::new(store, clock);
    assert_ne!(svc2.status(), AuthStatus::Unlocked);
    assert!(svc2.login(&u, &p).is_err(), "凭据库不可用时不放行");
    assert_ne!(svc2.status(), AuthStatus::NeedsSetup, "不能因读不到而回到首次设置（会被用来绕过）");
}

#[test]
fn gate_blocks_host_list_and_sessions_until_unlocked() {
    use my_terminal_lib::auth::require_unlocked;
    let (_s, _c, svc) = fresh();
    assert!(require_unlocked(&svc).is_err());
    let p = rand_token("p");
    svc.setup(&rand_token("u"), &p, &p).unwrap();
    assert!(require_unlocked(&svc).is_ok());
    svc.lock();
    assert!(require_unlocked(&svc).is_err());
}

#[allow(dead_code)]
fn _store_is_object_safe(_: &dyn SecretStore) {}

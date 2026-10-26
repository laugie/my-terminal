// #37 失败测试：点窗口关闭按钮、Alt+F4 或 Cmd+Q 时，有会话就先让网页弹确认框。
// 网页没有回报弹窗（卡死）时，3 秒后再点一次关闭直接退出，不会把应用困住。
use std::time::{Duration, Instant};

use my_terminal_lib::quit::{decide, CloseDecision, Prompt, PROMPT_GRACE};

#[test]
fn grace_is_three_seconds() {
    assert_eq!(PROMPT_GRACE, Duration::from_secs(3));
}

#[test]
fn no_sessions_close_at_once() {
    assert_eq!(decide(false, 0, None, Instant::now()), CloseDecision::Allow);
}

#[test]
fn sessions_ask_first() {
    assert_eq!(decide(false, 2, None, Instant::now()), CloseDecision::Ask);
}

#[test]
fn a_confirmed_quit_closes() {
    assert_eq!(decide(true, 2, None, Instant::now()), CloseDecision::Allow);
}

#[test]
fn a_dialog_that_was_shown_and_cancelled_asks_again() {
    let now = Instant::now();
    let shown = Prompt { since: now - Duration::from_secs(60), shown: true };
    assert_eq!(decide(false, 1, Some(shown), now), CloseDecision::Ask);
}

#[test]
fn a_recent_unanswered_request_asks_again() {
    let now = Instant::now();
    let pending = Prompt { since: now - Duration::from_millis(500), shown: false };
    assert_eq!(decide(false, 1, Some(pending), now), CloseDecision::Ask);
}

#[test]
fn a_page_that_never_showed_the_dialog_does_not_trap_the_app() {
    let now = Instant::now();
    let stale = Prompt { since: now - Duration::from_secs(4), shown: false };
    assert_eq!(decide(false, 1, Some(stale), now), CloseDecision::Allow);
}

//! Failure delay and an injectable clock.
//!
//! The 5th failure waits 30s. Each later failure doubles that, up to 15 minutes.
//! A wall-clock rollback must not make the remaining wait shorter.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Failures that are still answered immediately. The 5th failure starts the wait.
const FREE_FAILURES: u32 = 4;

const BASE_WAIT: Duration = Duration::from_secs(30);

/// Longest wait after a failure.
pub const MAX_WAIT: Duration = Duration::from_secs(15 * 60);

/// How long the next attempt must wait after `failures` bad logins.
///
/// `None` means the attempt that just failed does not start a wait.
pub fn delay_for_failures(failures: u32) -> Option<Duration> {
    if failures <= FREE_FAILURES {
        return None;
    }
    let steps = (failures - FREE_FAILURES - 1).min(5);
    let seconds = BASE_WAIT.as_secs().saturating_mul(1u64 << steps);
    Some(Duration::from_secs(seconds.min(MAX_WAIT.as_secs())))
}

/// Persisted unlock deadline. Times are milliseconds relative to the Unix epoch
/// and may be negative if a test clock is rewound past the epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    pub not_before_ms: i64,
    pub last_seen_ms: i64,
}

/// Fold a clock rollback into the deadline so the remaining wait stays the same.
///
/// Forward movement leaves `not_before_ms` alone, so the wait shrinks as time
/// actually passes. `last_seen_ms` is always moved to `now_ms`.
pub fn observe(deadline: &mut Deadline, now_ms: i64) {
    if now_ms < deadline.last_seen_ms {
        let remain = deadline
            .not_before_ms
            .saturating_sub(deadline.last_seen_ms)
            .max(0);
        deadline.not_before_ms = now_ms.saturating_add(remain);
    }
    deadline.last_seen_ms = now_ms;
}

/// Remaining wait at `now_ms`, including a rollback that has not been stored yet.
pub fn remaining(deadline: &Deadline, now_ms: i64) -> Duration {
    let not_before = if now_ms < deadline.last_seen_ms {
        let remain = deadline
            .not_before_ms
            .saturating_sub(deadline.last_seen_ms)
            .max(0);
        now_ms.saturating_add(remain)
    } else {
        deadline.not_before_ms
    };
    let delta = not_before.saturating_sub(now_ms);
    if delta <= 0 {
        Duration::ZERO
    } else {
        Duration::from_millis(delta as u64)
    }
}

pub fn duration_ms(delay: Duration) -> i64 {
    i64::try_from(delay.as_millis()).unwrap_or(i64::MAX)
}

/// Time source used by [`super::AuthService`].
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

/// Clock that tests move by hand. It never sleeps.
#[derive(Debug)]
pub struct FakeClock {
    now_ms: Mutex<i64>,
}

impl Default for FakeClock {
    fn default() -> Self {
        Self {
            now_ms: Mutex::new(1_700_000_000_000),
        }
    }
}

impl FakeClock {
    pub fn advance(&self, by: Duration) {
        let mut now = self.now_ms.lock().unwrap_or_else(|err| err.into_inner());
        *now = now.saturating_add(duration_ms(by));
    }

    pub fn rewind(&self, by: Duration) {
        let mut now = self.now_ms.lock().unwrap_or_else(|err| err.into_inner());
        *now = now.saturating_sub(duration_ms(by));
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        *self.now_ms.lock().unwrap_or_else(|err| err.into_inner())
    }
}

/// Wall clock used by the running app.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(elapsed) => i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
            Err(err) => -i64::try_from(err.duration().as_millis()).unwrap_or(i64::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_and_then_stop_at_fifteen_minutes() {
        assert_eq!(delay_for_failures(4), None);
        assert_eq!(delay_for_failures(5), Some(BASE_WAIT));
        assert_eq!(delay_for_failures(6), Some(Duration::from_secs(60)));
        assert_eq!(delay_for_failures(7), Some(Duration::from_secs(120)));
        assert_eq!(delay_for_failures(8), Some(Duration::from_secs(240)));
        assert_eq!(delay_for_failures(9), Some(Duration::from_secs(480)));
        assert_eq!(delay_for_failures(10), Some(MAX_WAIT));
        assert_eq!(delay_for_failures(40), Some(MAX_WAIT));
        assert!(MAX_WAIT <= Duration::from_secs(15 * 60));
    }

    #[test]
    fn rollback_preserves_remaining_instead_of_shrinking_it() {
        let mut deadline = Deadline {
            not_before_ms: 30_000,
            last_seen_ms: 0,
        };
        observe(&mut deadline, 0);
        deadline.not_before_ms = 30_000;
        let before = remaining(&deadline, 0);
        assert_eq!(before, Duration::from_secs(30));
        let rolled = -3_600_000;
        let after = remaining(&deadline, rolled);
        assert_eq!(after, before);
        observe(&mut deadline, rolled);
        assert_eq!(remaining(&deadline, rolled), before);
    }
}

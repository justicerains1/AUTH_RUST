//! Injectable wall clock for expiry rules; tests use exact timestamps, never expiry sleeps.
use time::OffsetDateTime;

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FixedClock {
    instant: OffsetDateTime,
}

impl FixedClock {
    pub const fn new(instant: OffsetDateTime) -> Self {
        Self { instant }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.instant
    }
}

/// Exclusive expiry: an action expiring at `now` is already invalid.
pub fn is_unexpired(clock: &dyn Clock, expires_at: OffsetDateTime) -> bool {
    expires_at > clock.now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    #[test]
    fn expiry_is_exclusive_and_independent_of_wall_clock() {
        let instant = OffsetDateTime::UNIX_EPOCH + Duration::days(20_000);
        let clock = FixedClock::new(instant);
        assert!(is_unexpired(&clock, instant + Duration::nanoseconds(1)));
        assert!(!is_unexpired(&clock, instant));
        assert!(!is_unexpired(&clock, instant - Duration::nanoseconds(1)));
        assert_eq!(clock.now(), instant);
    }
}

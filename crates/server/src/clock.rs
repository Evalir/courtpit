//! An injectable source of "now", so time-driven behaviour (deadlines, season transitions,
//! job scheduling) can be tested by moving the clock instead of sleeping.
//!
//! Rule: business logic that compares against the current time reads [`Clock::now`] and binds
//! it into SQL; it does not use Postgres `now()`. Audit columns (`created_at`, ...) may keep
//! their `DEFAULT now()`.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};

/// Source of the current time.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// The current instant.
    fn now(&self) -> DateTime<Utc>;
}

/// Wall-clock time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Wall-clock time shifted by an adjustable offset (tests jump forward with [`advance`]).
///
/// At offset zero it agrees with the database's `now()`, so tests that don't care about time
/// behave exactly as with [`SystemClock`].
///
/// [`advance`]: OffsetClock::advance
#[derive(Debug, Default)]
pub struct OffsetClock {
    offset: Mutex<Duration>,
}

impl OffsetClock {
    /// A clock at offset zero, shared.
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Moves the clock forward by `by` (negative moves it back).
    pub fn advance(&self, by: Duration) {
        if let Ok(mut offset) = self.offset.lock() {
            *offset += by;
        }
    }

    /// Moves the clock so that `now()` is (approximately) `at`.
    pub fn set(&self, at: DateTime<Utc>) {
        if let Ok(mut offset) = self.offset.lock() {
            *offset = at - Utc::now();
        }
    }
}

impl Clock for OffsetClock {
    fn now(&self) -> DateTime<Utc> {
        let offset = self.offset.lock().map(|offset| *offset).unwrap_or_default();
        Utc::now() + offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_clock_moves() {
        let clock = OffsetClock::default();
        let before = Utc::now();
        assert!(clock.now() >= before);
        clock.advance(Duration::days(3));
        assert!(clock.now() >= before + Duration::days(3));
        let target = before + Duration::days(100);
        clock.set(target);
        assert!((clock.now() - target).num_seconds().abs() < 2);
    }
}

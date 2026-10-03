//! Per-key rate limiting (in memory, per instance; good enough for abuse guardrails), backed by
//! `governor`'s keyed GCRA limiter.

use std::{num::NonZeroU32, sync::Arc};

use governor::{DefaultKeyedRateLimiter, Quota};

use crate::ApiError;

/// Keys tracked before stale ones are swept (so a flood of distinct keys cannot grow memory).
const SWEEP_ABOVE: usize = 100_000;

/// Allows a burst of `per_hour` hits per key, refilling one hit every `1h / per_hour`.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<DefaultKeyedRateLimiter<String>>,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter").finish_non_exhaustive()
    }
}

impl RateLimiter {
    /// A limiter allowing `per_hour` hits per key per hour (a limit of 0 is treated as 1).
    pub fn per_hour(per_hour: u32) -> Self {
        let quota = Quota::per_hour(NonZeroU32::new(per_hour).unwrap_or(NonZeroU32::MIN));
        Self {
            inner: Arc::new(DefaultKeyedRateLimiter::keyed(quota)),
        }
    }

    /// Records a hit for `key`; errors once the key has exhausted its quota.
    pub fn check(&self, key: &str) -> Result<(), ApiError> {
        if self.inner.len() > SWEEP_ABOVE {
            self.inner.retain_recent();
        }
        self.inner
            .check_key(&key.to_owned())
            .map_err(|_| ApiError::RateLimited)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_per_key() {
        let rl = RateLimiter::per_hour(2);
        rl.check("a").unwrap();
        rl.check("a").unwrap();
        assert!(rl.check("a").is_err());
        rl.check("b").unwrap();
    }
}

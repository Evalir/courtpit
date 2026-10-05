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

/// The startup warning that per-IP auth rate limits live in each instance's memory, or `None`
/// when there is nothing to warn about. `fly_machine_id` is the `FLY_MACHINE_ID` environment
/// variable, set inside every Fly Machine. Whether more Machines run alongside this one is not
/// knowable from the inside, so being on Fly is the whole signal.
pub fn rate_limit_notice(fly_machine_id: Option<&str>) -> Option<String> {
    let machine = fly_machine_id.map(str::trim).filter(|id| !id.is_empty())?;
    Some(format!(
        "running on Fly Machine {machine}: auth rate limits are per instance, so N Machines allow \
         N times RACQUETCOLLECTIVE_AUTH_IP_LIMIT_PER_HOUR; keep one Machine or move the limits to Postgres"
    ))
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

    #[test]
    fn warns_only_on_fly() {
        assert_eq!(rate_limit_notice(None), None);
        assert_eq!(rate_limit_notice(Some("")), None);
        assert_eq!(rate_limit_notice(Some("  ")), None);
        let notice = rate_limit_notice(Some("148e21d3a9e418")).unwrap();
        assert!(notice.contains("148e21d3a9e418"), "{notice}");
        assert!(notice.contains("per instance"), "{notice}");
        assert!(
            notice.contains("RACQUETCOLLECTIVE_AUTH_IP_LIMIT_PER_HOUR"),
            "{notice}"
        );
    }
}

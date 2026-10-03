//! In-memory fixed-window rate limiting (per instance; good enough for abuse guardrails).

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use moka::sync::Cache;

use crate::ApiError;

/// Counts hits per key in fixed windows of `window`.
#[derive(Clone)]
pub struct RateLimiter {
    hits: Cache<String, Arc<AtomicU32>>,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter").finish_non_exhaustive()
    }
}

impl RateLimiter {
    /// A limiter whose counters reset `window` after the first hit.
    pub fn new(window: Duration) -> Self {
        Self {
            hits: Cache::builder()
                .max_capacity(100_000)
                .time_to_live(window)
                .build(),
        }
    }

    /// Records a hit for `key`; errors once more than `limit` hits land in the window.
    pub fn check(&self, key: &str, limit: u32) -> Result<(), ApiError> {
        let counter = self
            .hits
            .get_with(key.to_owned(), || Arc::new(AtomicU32::new(0)));
        if counter.fetch_add(1, Ordering::Relaxed) >= limit {
            return Err(ApiError::RateLimited);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_per_key() {
        let rl = RateLimiter::new(Duration::from_secs(60));
        rl.check("a", 2).unwrap();
        rl.check("a", 2).unwrap();
        assert!(rl.check("a", 2).is_err());
        rl.check("b", 2).unwrap();
    }
}

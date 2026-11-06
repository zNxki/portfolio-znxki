use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check(&self, key: &str, limit: u32, window: Duration) -> Result<(), u64> {
        self.check_at(key, limit, window, Instant::now())
    }

    pub fn check_at(&self, key: &str, limit: u32, window: Duration, now: Instant) -> Result<(), u64> {
        let mut guard = match self.buckets.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let entry = guard.entry(key.to_string()).or_default();
        while let Some(front) = entry.front() {
            if now.duration_since(*front) >= window {
                entry.pop_front();
            } else {
                break;
            }
        }
        if entry.len() >= limit as usize {
            let retry = entry
                .front()
                .map(|f| window.saturating_sub(now.duration_since(*f)).as_secs() + 1)
                .unwrap_or(1);
            return Err(retry);
        }
        entry.push_back(now);
        Ok(())
    }

    pub fn prune(&self, window: Duration) {
        let now = Instant::now();
        let mut guard = match self.buckets.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.retain(|_, q| match q.back() {
            Some(last) => now.duration_since(*last) < window,
            None => false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_limit_and_recovers() {
        let rl = RateLimiter::new();
        let w = Duration::from_secs(60);
        let t0 = Instant::now();
        assert!(rl.check_at("k", 2, w, t0).is_ok());
        assert!(rl.check_at("k", 2, w, t0).is_ok());
        assert!(rl.check_at("k", 2, w, t0).is_err());
        assert!(rl.check_at("other", 2, w, t0).is_ok());
        assert!(rl.check_at("k", 2, w, t0 + Duration::from_secs(61)).is_ok());
    }

    #[test]
    fn reports_retry_after() {
        let rl = RateLimiter::new();
        let w = Duration::from_secs(10);
        let t0 = Instant::now();
        assert!(rl.check_at("k", 1, w, t0).is_ok());
        match rl.check_at("k", 1, w, t0 + Duration::from_secs(3)) {
            Err(retry) => assert!(retry >= 7 && retry <= 8),
            Ok(_) => panic!("doveva essere bloccato"),
        }
    }
}
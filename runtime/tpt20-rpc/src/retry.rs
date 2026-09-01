//! Retry policy for resilient RPC calls (spec §16).

use std::time::Duration;
use crate::status::Status;

#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub backoff_multiplier: f64,
    pub retryable_statuses: Vec<Status>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            retryable_statuses: vec![Status::Unavailable, Status::Aborted, Status::Internal],
        }
    }
}

impl RetryPolicy {
    pub fn new() -> Self { Self::default() }
    pub fn is_retryable(&self, status: Status) -> bool {
        self.retryable_statuses.contains(&status)
    }
    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let backoff = self.initial_backoff.mul_f64(self.backoff_multiplier.powi(attempt as i32));
        backoff.min(self.max_backoff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_policy_retries_unavailable() {
        let policy = RetryPolicy::default();
        assert!(policy.is_retryable(Status::Unavailable));
        assert!(!policy.is_retryable(Status::InvalidArgument));
    }
    #[test]
    fn backoff_grows_exponentially() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.backoff_for_attempt(0), Duration::from_millis(100));
        assert_eq!(policy.backoff_for_attempt(1), Duration::from_millis(200));
    }
    #[test]
    fn backoff_capped_at_max() {
        let policy = RetryPolicy {
            initial_backoff: Duration::from_secs(10),
            max_backoff: Duration::from_secs(15),
            ..RetryPolicy::default()
        };
        assert_eq!(policy.backoff_for_attempt(1), Duration::from_secs(15));
    }
}

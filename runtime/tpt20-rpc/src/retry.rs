//! Retry policy for resilient RPC calls (spec §16).

use std::time::Duration;

use crate::status::Status;

/// Configuration for retrying failed RPCs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial backoff duration before the first retry.
    pub initial_backoff: Duration,
    /// Maximum backoff duration between retries.
    pub max_backoff: Duration,
    /// Multiplier applied to the backoff after each attempt.
    pub backoff_multiplier: f64,
    /// Status codes that trigger a retry.
    pub retryable_statuses: Vec<Status>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            retryable_statuses: vec![
                Status::Unavailable,
                Status::Aborted,
                Status::Internal,
            ],
        }
    }
}

impl RetryPolicy {
    /// Creates a new retry policy with sensible defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the given status is retryable under this policy.
    pub fn is_retryable(&self, status: Status) -> bool {
        self.retryable_statuses.contains(&status)
    }

    /// Computes the backoff duration for the given attempt number (0-based).
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
        assert_eq!(policy.backoff_for_attempt(2), Duration::from_millis(400));
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

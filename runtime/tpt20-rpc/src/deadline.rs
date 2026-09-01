//! Deadline management for RPC calls (spec §16.1).

use std::time::{Duration, Instant};

/// A point in time by which an RPC must complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deadline {
    instant: Instant,
}

impl Deadline {
    /// Creates a deadline `duration` from now.
    pub fn from_now(duration: Duration) -> Self {
        Self {
            instant: Instant::now() + duration,
        }
    }

    /// Creates a deadline from an absolute instant.
    pub const fn from_instant(instant: Instant) -> Self {
        Self { instant }
    }

    /// Returns true if the deadline has passed.
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.instant
    }

    /// Returns the remaining time until the deadline, or zero if expired.
    pub fn remaining_time(&self) -> Duration {
        self.instant.saturating_duration_since(Instant::now())
    }

    /// Returns the absolute instant for this deadline.
    pub const fn instant(&self) -> Instant {
        self.instant
    }
}

impl Default for Deadline {
    fn default() -> Self {
        Self {
            instant: Instant::now() + Duration::from_secs(60),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn deadline_from_now_not_expired() {
        let d = Deadline::from_now(Duration::from_secs(10));
        assert!(!d.is_expired());
        assert!(d.remaining_time() < Duration::from_secs(10));
    }

    #[test]
    fn deadline_from_past_is_expired() {
        let past = Instant::now() - Duration::from_secs(1);
        let d = Deadline::from_instant(past);
        assert!(d.is_expired());
        assert_eq!(d.remaining_time(), Duration::ZERO);
    }
}

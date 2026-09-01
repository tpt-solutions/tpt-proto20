//! Deadline management for RPC calls (spec §16.1).

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deadline {
    instant: Instant,
}

impl Deadline {
    pub fn from_now(duration: Duration) -> Self {
        Self { instant: Instant::now() + duration }
    }
    pub const fn from_instant(instant: Instant) -> Self { Self { instant } }
    pub fn is_expired(&self) -> bool { Instant::now() >= self.instant }
    pub fn remaining_time(&self) -> Duration {
        self.instant.saturating_duration_since(Instant::now())
    }
    pub const fn instant(&self) -> Instant { self.instant }
}

impl Default for Deadline {
    fn default() -> Self {
        Self { instant: Instant::now() + Duration::from_secs(60) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadline_from_now_not_expired() {
        let d = Deadline::from_now(Duration::from_secs(10));
        assert!(!d.is_expired());
    }
    #[test]
    fn deadline_from_past_is_expired() {
        let past = Instant::now() - Duration::from_secs(1);
        let d = Deadline::from_instant(past);
        assert!(d.is_expired());
        assert_eq!(d.remaining_time(), Duration::ZERO);
    }
}

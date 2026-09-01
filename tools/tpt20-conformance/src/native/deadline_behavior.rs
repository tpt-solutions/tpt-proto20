use std::time::Duration;
use tpt20_rpc::Deadline;

#[test]
fn deadline_from_now_not_expired() {
    let d = Deadline::from_now(Duration::from_secs(10));
    assert!(!d.is_expired());
}

#[test]
fn deadline_from_past_is_expired() {
    let past = std::time::Instant::now() - Duration::from_secs(1);
    let d = Deadline::from_instant(past);
    assert!(d.is_expired());
    assert_eq!(d.remaining_time(), Duration::ZERO);
}

#[test]
fn deadline_default_is_not_expired() {
    let d = Deadline::default();
    assert!(!d.is_expired());
}

#[test]
fn deadline_instant_accessor() {
    let d = Deadline::from_now(Duration::from_secs(10));
    let instant = d.instant();
    assert!(instant > std::time::Instant::now());
}

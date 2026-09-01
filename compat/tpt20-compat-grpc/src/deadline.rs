//! Deadline mapping between tpt20 and gRPC (spec §10.3).
//!
//! gRPC uses the `grpc-timeout` HTTP/2 header to express deadlines. This module
//! parses and serializes that header value and translates it to/from tpt20
//! [`tpt20_rpc::Deadline`].

use std::time::Duration;

use tpt20_rpc::Deadline;

/// Parses a `grpc-timeout` header value into a [`Duration`].
///
/// gRPC timeout format: `<value><unit>` where unit is one of:
/// - `n` — nanoseconds
/// - `u` — microseconds
/// - `m` — milliseconds
/// - `S` — seconds
/// - `M` — minutes
/// - `H` — hours
///
/// Returns an error if the value is empty, the number is not a valid
/// unsigned integer, or the unit is unknown.
pub fn parse_grpc_timeout(value: &str) -> Result<Duration, crate::GrpcError> {
    if value.is_empty() {
        return Err(crate::GrpcError::InvalidTimeout(value.into()));
    }
    let unit = value.chars().last().ok_or_else(|| {
        crate::GrpcError::InvalidTimeout(value.into())
    })?;
    let num_str = &value[..value.len() - 1];
    let num: u64 = num_str.parse().map_err(|_| {
        crate::GrpcError::InvalidTimeout(value.into())
    })?;
    match unit {
        'n' => Ok(Duration::from_nanos(num)),
        'u' => Ok(Duration::from_micros(num)),
        'm' => Ok(Duration::from_millis(num)),
        'S' => Ok(Duration::from_secs(num)),
        'M' => Ok(Duration::from_secs(num * 60)),
        'H' => Ok(Duration::from_secs(num * 3600)),
        _ => Err(crate::GrpcError::InvalidTimeout(value.into())),
    }
}

/// Serializes a [`Duration`] as a `grpc-timeout` header value.
///
/// Chooses the largest unit that can represent the duration without loss.
pub fn format_grpc_timeout(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    if nanos < 1_000 {
        format!("{}n", nanos)
    } else if nanos < 1_000_000 {
        format!("{}u", nanos / 1_000)
    } else if nanos < 1_000_000_000 {
        format!("{}m", nanos / 1_000_000)
    } else {
        let secs = nanos / 1_000_000_000;
        if secs < 60 {
            format!("{}S", secs)
        } else if secs < 3600 {
            format!("{}M", secs / 60)
        } else {
            format!("{}H", secs / 3600)
        }
    }
}

/// Creates a tpt20 [`Deadline`] from a `grpc-timeout` header value.
///
/// Returns `None` if the header is absent or empty, in which case the
/// default deadline should be used.
pub fn deadline_from_grpc_timeout(value: &str) -> Result<Option<Deadline>, crate::GrpcError> {
    if value.is_empty() {
        return Ok(None);
    }
    let duration = parse_grpc_timeout(value)?;
    Ok(Some(Deadline::from_now(duration)))
}

/// Creates a `grpc-timeout` header value from a tpt20 [`Deadline`].
///
/// Returns `None` if the deadline has already expired.
pub fn grpc_timeout_from_deadline(deadline: &Deadline) -> Option<String> {
    if deadline.is_expired() {
        return None;
    }
    let remaining = deadline.remaining_time();
    if remaining.is_zero() {
        return None;
    }
    Some(format_grpc_timeout(remaining))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parse_nanoseconds() {
        assert_eq!(parse_grpc_timeout("500n"), Ok(Duration::from_nanos(500)));
    }

    #[test]
    fn parse_microseconds() {
        assert_eq!(parse_grpc_timeout("100u"), Ok(Duration::from_micros(100)));
    }

    #[test]
    fn parse_milliseconds() {
        assert_eq!(parse_grpc_timeout("250m"), Ok(Duration::from_millis(250)));
    }

    #[test]
    fn parse_seconds() {
        assert_eq!(parse_grpc_timeout("10S"), Ok(Duration::from_secs(10)));
    }

    #[test]
    fn parse_minutes() {
        assert_eq!(parse_grpc_timeout("2M"), Ok(Duration::from_secs(120)));
    }

    #[test]
    fn parse_hours() {
        assert_eq!(parse_grpc_timeout("1H"), Ok(Duration::from_secs(3600)));
    }

    #[test]
    fn parse_invalid_unit() {
        assert!(parse_grpc_timeout("10X").is_err());
    }

    #[test]
    fn parse_empty() {
        assert!(parse_grpc_timeout("").is_err());
    }

    #[test]
    fn parse_non_numeric() {
        assert!(parse_grpc_timeout("abcS").is_err());
    }

    #[test]
    fn format_roundtrip() {
        for (value, expected) in [
            (Duration::from_nanos(500), "500n"),
            (Duration::from_micros(100), "100u"),
            (Duration::from_millis(250), "250m"),
            (Duration::from_secs(10), "10S"),
            (Duration::from_secs(120), "2M"),
            (Duration::from_secs(3600), "1H"),
        ] {
            let formatted = format_grpc_timeout(value);
            assert_eq!(formatted, expected);
            let parsed = parse_grpc_timeout(&formatted).unwrap();
            assert_eq!(parsed, value);
        }
    }
}

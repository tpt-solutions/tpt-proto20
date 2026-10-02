//! Wire conventions shared by RPC clients and servers.
//!
//! - The final status travels in the trailers as `grpc-status` (numeric
//!   code) and `grpc-message` (percent-encoded text). The keys deliberately
//!   match gRPC so gRPC-style clients can read tpt20 servers. A response
//!   that ends without `grpc-status` is an error, never success.
//! - The deadline travels as `grpc-timeout` (`<digits><unit>`, units
//!   `H M S m u n`).
//! - Binary metadata values use keys ending in `-bin` and are base64 on the
//!   wire.

use crate::error::RpcError;
use crate::metadata::Metadata;
use crate::status::Status;
use std::time::Duration;
use tpt20_transport::{Metadata as WireMetadata, TransportError};

/// Trailer key carrying the numeric status code.
pub const STATUS_KEY: &str = "grpc-status";
/// Trailer key carrying the percent-encoded status message.
pub const MESSAGE_KEY: &str = "grpc-message";
/// Request header carrying the remaining deadline.
pub const TIMEOUT_KEY: &str = "grpc-timeout";
/// W3C trace-context headers carrying the call's [`TraceContext`].
pub const TRACEPARENT_KEY: &str = "traceparent";
/// W3C `tracestate` header.
pub const TRACESTATE_KEY: &str = "tracestate";

/// Keys owned by the protocol; never surfaced as user metadata.
const RESERVED: &[&str] = &[
    STATUS_KEY,
    MESSAGE_KEY,
    TIMEOUT_KEY,
    "content-type",
    "te",
    "grpc-encoding",
    "grpc-accept-encoding",
    TRACEPARENT_KEY,
    TRACESTATE_KEY,
];

/// Percent-encodes a status message (bytes outside printable ASCII and `%`).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if (0x20..=0x7e).contains(&b) && b != b'%' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decodes [`percent_encode`]; malformed escapes are kept literally.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok());
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encodes a timeout as `grpc-timeout` (at most 8 digits, rounded up).
pub fn encode_timeout(d: Duration) -> String {
    const MAX: u128 = 99_999_999;
    let nanos = d.as_nanos();
    for (unit, per) in [
        ('n', 1u128),
        ('u', 1_000),
        ('m', 1_000_000),
        ('S', 1_000_000_000),
        ('M', 60_000_000_000),
    ] {
        let n = nanos.div_ceil(per);
        if n <= MAX {
            return format!("{n}{unit}");
        }
    }
    format!("{}H", nanos.div_ceil(3_600_000_000_000).min(MAX))
}

/// Parses `grpc-timeout`.
pub fn decode_timeout(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (digits, unit) = s.split_at(s.len().checked_sub(1)?);
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = digits.parse().ok()?;
    Some(match unit {
        "n" => Duration::from_nanos(n),
        "u" => Duration::from_micros(n),
        "m" => Duration::from_millis(n),
        "S" => Duration::from_secs(n),
        "M" => Duration::from_secs(n * 60),
        "H" => Duration::from_secs(n * 3600),
        _ => return None,
    })
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Formats a W3C `traceparent` (`00-<trace-id>-<span-id>-<flags>`); `None`
/// when the context is empty or its ids are not valid lowercase hex.
pub fn encode_traceparent(t: &crate::trace::TraceContext) -> Option<String> {
    if !is_hex(&t.trace_id, 32) || !is_hex(&t.span_id, 16) {
        return None;
    }
    if t.trace_id.bytes().all(|b| b == b'0') || t.span_id.bytes().all(|b| b == b'0') {
        return None;
    }
    Some(format!(
        "00-{}-{}-{:02x}",
        t.trace_id, t.span_id, t.trace_flags
    ))
}

/// Parses a W3C `traceparent` header (version `00`); `None` if malformed.
pub fn decode_traceparent(s: &str) -> Option<crate::trace::TraceContext> {
    let mut parts = s.trim().split('-');
    let (version, trace, span, flags) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || version != "00" || !is_hex(trace, 32) || !is_hex(span, 16) {
        return None;
    }
    if trace.bytes().all(|b| b == b'0') || span.bytes().all(|b| b == b'0') || !is_hex(flags, 2) {
        return None;
    }
    Some(crate::trace::TraceContext::new(
        trace,
        span,
        u8::from_str_radix(flags, 16).ok()?,
    ))
}

/// Converts RPC metadata to transport metadata (binary values → base64).
pub fn to_wire_metadata(md: &Metadata) -> WireMetadata {
    let mut out = WireMetadata::new();
    for (k, v) in md.iter() {
        match v {
            crate::metadata::MetadataValue::Text(t) => out.insert(k.as_str(), t.clone()),
            crate::metadata::MetadataValue::Binary(b) => {
                out.insert(k.as_str(), tpt20_json::base64::encode(b))
            }
        }
    }
    out
}

/// Converts transport metadata to RPC metadata, dropping protocol keys.
pub fn from_wire_metadata(md: &WireMetadata) -> Result<Metadata, RpcError> {
    let mut out = Metadata::with_default_limit();
    for (k, values) in md.iter() {
        if RESERVED.contains(&k) || k.starts_with(':') {
            continue;
        }
        let joined = values.join(",");
        let res = if k.ends_with("-bin") {
            match tpt20_json::base64::decode(&joined) {
                Ok(bytes) => out.insert_binary(k, bytes),
                Err(_) => {
                    return Err(RpcError::invalid_argument(format!(
                        "metadata `{k}` is not valid base64"
                    ))
                    .finish())
                }
            }
        } else {
            out.insert_text(k, joined)
        };
        res.map_err(|e| RpcError::resource_exhausted(format!("metadata rejected: {e}")).finish())?;
    }
    Ok(out)
}

/// Builds the final trailers for a call outcome.
pub fn status_trailers(result: &Result<(), RpcError>) -> WireMetadata {
    let mut t = WireMetadata::new();
    match result {
        Ok(()) => t.insert(STATUS_KEY, "0"),
        Err(e) => {
            t.insert(STATUS_KEY, e.status().code().to_string());
            if !e.message().is_empty() {
                t.insert(MESSAGE_KEY, percent_encode(e.message()));
            }
        }
    }
    t
}

/// Reads the call outcome from response trailers.
pub fn parse_status(trailers: &WireMetadata) -> Result<(), RpcError> {
    let first = |key: &str| trailers.get(key).and_then(|v| v.first()).cloned();
    let Some(code) = first(STATUS_KEY) else {
        return Err(RpcError::unknown("response ended without grpc-status").finish());
    };
    let status = code
        .trim()
        .parse::<i32>()
        .ok()
        .and_then(Status::from_code)
        .ok_or_else(|| RpcError::unknown(format!("invalid grpc-status `{code}`")).finish())?;
    if status == Status::Ok {
        return Ok(());
    }
    let message = first(MESSAGE_KEY)
        .map(|m| percent_decode(&m))
        .unwrap_or_default();
    Err(RpcError::new(status, message))
}

/// Maps a transport failure onto an RPC status.
pub fn transport_error(e: TransportError) -> RpcError {
    let status = match &e {
        TransportError::SizeLimitExceeded { .. } => Status::ResourceExhausted,
        TransportError::StreamReset => Status::Cancelled,
        TransportError::ConnectionClosed
        | TransportError::GoAway(_)
        | TransportError::Io(_)
        | TransportError::Tls(_) => Status::Unavailable,
        TransportError::MalformedFrame(_) => Status::Internal,
        _ => Status::Internal,
    };
    RpcError::new(status, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_roundtrip_and_limits() {
        for d in [
            Duration::from_nanos(5),
            Duration::from_millis(250),
            Duration::from_secs(10),
            Duration::from_secs(3 * 3600),
            Duration::from_secs(400_000),
        ] {
            let enc = encode_timeout(d);
            assert!(enc.len() <= 9, "{enc}");
            let dec = decode_timeout(&enc).unwrap();
            assert!(
                dec >= d && dec - d <= d / 1000 + Duration::from_nanos(1),
                "{enc} {dec:?}"
            );
        }
        assert_eq!(decode_timeout("100m"), Some(Duration::from_millis(100)));
        for bad in ["", "m", "12", "1x", "123456789S", "-1S", "1.5S"] {
            assert_eq!(decode_timeout(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn status_trailers_roundtrip() {
        assert_eq!(parse_status(&status_trailers(&Ok(()))), Ok(()));
        let err = RpcError::not_found("no such user: é%").finish();
        assert_eq!(parse_status(&status_trailers(&Err(err.clone()))), Err(err));
        let e = parse_status(&WireMetadata::new()).unwrap_err();
        assert_eq!(e.status(), Status::Unknown);
        let mut bad = WireMetadata::new();
        bad.insert(STATUS_KEY, "99");
        assert_eq!(parse_status(&bad).unwrap_err().status(), Status::Unknown);
    }

    #[test]
    fn metadata_roundtrip_with_binary_and_reserved_keys() {
        let mut md = Metadata::with_default_limit();
        md.insert_text("x-user", "ada").unwrap();
        md.insert_binary("x-blob-bin", vec![0u8, 255, 7]).unwrap();
        let mut wire = to_wire_metadata(&md);
        wire.insert(TIMEOUT_KEY, "5S");
        wire.insert("content-type", "application/tpt20");
        wire.insert("grpc-encoding", "gzip");
        wire.insert("grpc-accept-encoding", "gzip,deflate");
        let back = from_wire_metadata(&wire).unwrap();
        assert_eq!(back.get_first_text("x-user"), Some("ada"));
        assert_eq!(
            back.get("x-blob-bin"),
            Some(&crate::metadata::MetadataValue::binary(vec![0u8, 255, 7]))
        );
        assert!(back.get(TIMEOUT_KEY).is_none() && back.get("content-type").is_none());
        assert!(back.get("grpc-encoding").is_none() && back.get("grpc-accept-encoding").is_none());

        let mut bad = WireMetadata::new();
        bad.insert("x-bad-bin", "!!!");
        assert_eq!(
            from_wire_metadata(&bad).unwrap_err().status(),
            Status::InvalidArgument
        );
    }

    #[test]
    fn traceparent_roundtrip_and_validation() {
        let t = crate::trace::TraceContext::new(
            "4bf92f3577b34da6a3ce929d0e0e4736",
            "00f067aa0ba902b7",
            1,
        );
        let header = encode_traceparent(&t).unwrap();
        assert_eq!(
            header,
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        );
        assert_eq!(decode_traceparent(&header), Some(t));
        // Empty/invalid contexts are not sent; malformed headers are ignored.
        assert_eq!(encode_traceparent(&Default::default()), None);
        assert_eq!(
            encode_traceparent(&crate::trace::TraceContext::new("trace-123", "span-456", 1)),
            None
        );
        for bad in [
            "",
            "garbage",
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
        ] {
            assert_eq!(decode_traceparent(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn transport_errors_map_to_statuses() {
        assert_eq!(
            transport_error(TransportError::ConnectionClosed).status(),
            Status::Unavailable
        );
        assert_eq!(
            transport_error(TransportError::SizeLimitExceeded { limit: 1 }).status(),
            Status::ResourceExhausted
        );
        assert_eq!(
            transport_error(TransportError::StreamReset).status(),
            Status::Cancelled
        );
    }
}

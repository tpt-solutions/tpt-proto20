//! Metadata mapping between tpt20 and gRPC (spec §10.3).
//!
//! gRPC metadata is carried in HTTP/2 headers. This module translates between
//! tpt20 [`Metadata`] and HTTP/2 header maps.

use crate::GrpcError;
use base64::Engine;
use tpt20_rpc::{Metadata, MetadataValue};

/// gRPC pseudo-headers that are never treated as metadata.
const GRPC_PSEUDO_HEADERS: &[&str] = &[
    ":authority",
    ":path",
    ":method",
    ":scheme",
    ":status",
];

/// gRPC protocol headers that are not application metadata.
const GRPC_PROTOCOL_HEADERS: &[&str] = &[
    "content-type",
    "grpc-timeout",
    "grpc-encoding",
    "grpc-accept-encoding",
    "grpc-message",
    "grpc-status",
];

/// Translates HTTP/2 headers into a tpt20 [`Metadata`].
///
/// gRPC pseudo-headers and protocol headers are excluded from the metadata.
/// All other headers become text metadata entries. Keys are lowercased as
/// required by tpt20 metadata.
pub fn from_grpc_headers(headers: &http::HeaderMap) -> Result<Metadata, GrpcError> {
    let mut md = Metadata::with_default_limit();
    for (key, value) in headers.iter() {
        let key_str = key.as_str();
        if GRPC_PSEUDO_HEADERS.contains(&key_str) {
            continue;
        }
        if GRPC_PROTOCOL_HEADERS.contains(&key_str) {
            continue;
        }
        let value_str = value
            .to_str()
            .map_err(|_| GrpcError::Metadata("non-UTF-8 header value".into()))?;
        md.insert_text(key_str, value_str)?;
    }
    Ok(md)
}

/// Translates HTTP/2 trailers into a tpt20 [`Metadata`].
///
/// gRPC trailers include `grpc-status` and `grpc-message`. These are protocol
/// trailers and are not included in the returned metadata.
pub fn from_grpc_trailers(trailers: &http::HeaderMap) -> Result<Metadata, GrpcError> {
    let mut md = Metadata::with_default_limit();
    for (key, value) in trailers.iter() {
        let key_str = key.as_str();
        if key_str == "grpc-status" || key_str == "grpc-message" {
            continue;
        }
        let value_str = value
            .to_str()
            .map_err(|_| GrpcError::Metadata("non-UTF-8 trailer value".into()))?;
        md.insert_text(key_str, value_str)?;
    }
    Ok(md)
}

/// Translates a tpt20 [`Metadata`] into HTTP/2 headers.
///
/// Text values are inserted as-is. Binary values are base64-encoded and
/// the key is suffixed with `-bin` if not already present.
pub fn to_grpc_headers(metadata: &Metadata) -> Result<http::HeaderMap, GrpcError> {
    let mut headers = http::HeaderMap::new();
    for (key, value) in metadata.iter() {
        let key_str = key.as_str();
        match value {
            MetadataValue::Text(v) => {
                let header_name: http::HeaderName = key_str.parse().map_err(|_| GrpcError::Metadata("invalid header name".into()))?;
                let header_value: http::HeaderValue = v.as_str().parse().map_err(|_| {
                    GrpcError::Metadata("invalid text metadata value".into())
                })?;
                headers.insert(header_name, header_value);
            }
            MetadataValue::Binary(v) => {
                let encoded = base64::engine::general_purpose::STANDARD.encode(v);
                let bin_key = if key_str.ends_with("-bin") {
                    key_str.to_string()
                } else {
                    format!("{}-bin", key_str)
                };
                let header_name: http::HeaderName = bin_key.parse().map_err(|_| GrpcError::Metadata("invalid header name".into()))?;
                let header_value: http::HeaderValue = encoded.parse().map_err(|_| GrpcError::Metadata("invalid base64 value".into()))?;
                headers.insert(header_name, header_value);
            }
        }
    }
    Ok(headers)
}

/// Translates a tpt20 [`Metadata`] into HTTP/2 trailers.
///
/// Used for response trailing metadata.
pub fn to_grpc_trailers(metadata: &Metadata) -> Result<http::HeaderMap, GrpcError> {
    to_grpc_headers(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_grpc_headers_preserves_regular_headers() {
        let mut headers = http::HeaderMap::new();
        headers.insert("x-request-id", "abc".parse().unwrap());
        headers.insert("x-trace", "trace123".parse().unwrap());
        let md = from_grpc_headers(&headers).unwrap();
        assert_eq!(md.get("x-request-id").map(|v| v.as_ref()), Some("abc"));
        assert_eq!(md.get("x-trace").map(|v| v.as_ref()), Some("trace123"));
    }

    #[test]
    fn from_grpc_headers_skips_protocol_headers() {
        let mut headers = http::HeaderMap::new();
        headers.insert("content-type", "application/grpc".parse().unwrap());
        headers.insert("grpc-timeout", "10S".parse().unwrap());
        headers.insert("x-trace", "trace123".parse().unwrap());
        let md = from_grpc_headers(&headers).unwrap();
        assert!(md.get("content-type").is_none());
        assert!(md.get("grpc-timeout").is_none());
        assert_eq!(md.get("x-trace").map(|v| v.as_ref()), Some("trace123"));
    }

    #[test]
    fn to_grpc_headers_encodes_binary() {
        let mut md = Metadata::new(1024);
        md.insert_binary("x-data-bin", b"hello").unwrap();
        let headers = to_grpc_headers(&md).unwrap();
        assert!(headers.contains_key("x-data-bin"));
    }
}

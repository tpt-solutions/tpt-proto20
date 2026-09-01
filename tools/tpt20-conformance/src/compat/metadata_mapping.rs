use tpt20_compat_grpc::{from_grpc_headers, from_grpc_trailers, to_grpc_headers};
use tpt20_rpc::Metadata;

#[test]
fn from_grpc_headers_skips_pseudo_headers() {
    let mut headers = http::HeaderMap::new();
    headers.insert(":authority", "example.com".parse().unwrap());
    headers.insert("x-request-id", "abc".parse().unwrap());
    let md = from_grpc_headers(&headers).unwrap();
    assert!(md.get(":authority").is_none());
    assert_eq!(md.get("x-request-id").map(|v| v.as_ref()), Some("abc"));
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

#[test]
fn metadata_insert_enforces_lowercase() {
    let mut md = Metadata::new(1024);
    assert!(md.insert_text("x-request-id", "abc").is_ok());
    assert!(md.insert_text("X-Request-Id", "def").is_err());
}

#[test]
fn from_grpc_trailers_skips_protocol() {
    let mut trailers = http::HeaderMap::new();
    trailers.insert("grpc-status", "0".parse().unwrap());
    trailers.insert("grpc-message", "OK".parse().unwrap());
    trailers.insert("x-custom", "value".parse().unwrap());
    let md = from_grpc_trailers(&trailers).unwrap();
    assert!(md.get("grpc-status").is_none());
    assert!(md.get("grpc-message").is_none());
    assert_eq!(md.get("x-custom").map(|v| v.as_ref()), Some("value"));
}

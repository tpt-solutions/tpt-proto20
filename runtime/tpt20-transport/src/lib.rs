//! `tpt20-transport`: transport layer for tpt20 RPC (spec §17).
//!
//! This crate provides:
//!
//! - Message framing: 1-byte flags + 4-byte big-endian length + N-byte payload
//! - Transport traits (transport-agnostic RPC interface)
//! - In-process transport (for tests, embedded, local dev, benchmarking, fuzzing)
//! - HTTP/2 transport (required production transport, feature-gated)
//! - QUIC/HTTP3 transport (optional, feature-gated)
//! - Custom stream transport extension point
//!
//! ## Feature flags
//!
//! - `default` = `["in_process"]` — in-process transport is always available
//! - `http2` — HTTP/2 production transport (requires `h2`)
//! - `tls` — TLS with ALPN (requires `tokio-rustls` and `rustls-pemfile`)
//! - `quic` — QUIC/HTTP3 transport (requires `quinn`)

pub mod endpoint;
pub mod error;
pub mod frame;
#[cfg(feature = "http2")]
pub mod http2;
pub mod in_process;
pub mod metadata;
pub mod traits;

pub use endpoint::{Endpoint, TlsConfig};
pub use error::TransportError;
pub use frame::{decode_frame, encode_frame, Frame, FrameFlags};
#[cfg(feature = "http2")]
pub use http2::{Http2Server, Http2Transport};
pub use in_process::{InProcessServer, InProcessTransport};
pub use metadata::Metadata;
pub use traits::{Call, StreamingType, Transport};

//! Error types for the transport layer.

use thiserror::Error;

/// Errors that can occur during transport operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransportError {
    /// The connection was closed unexpectedly.
    #[error("connection closed")]
    ConnectionClosed,

    /// A stream was reset by the peer.
    #[error("stream reset by peer")]
    StreamReset,

    /// The peer sent a GOAWAY frame.
    #[error("peer sent GOAWAY: {0}")]
    GoAway(String),

    /// A message frame was malformed.
    #[error("malformed frame: {0}")]
    MalformedFrame(String),

    /// A message exceeded the configured size limit.
    #[error("message size limit exceeded ({limit} bytes)")]
    SizeLimitExceeded {
        /// The limit that was violated.
        limit: usize,
    },

    /// A TLS error occurred.
    #[error("TLS error: {0}")]
    Tls(String),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(String),

    /// The stream is in an invalid state for the requested operation.
    #[error("invalid stream state: {0}")]
    InvalidState(String),

    /// A compression/decompression error occurred.
    #[error("compression error: {0}")]
    Compression(String),

    /// The transport is not supported or not enabled.
    #[error("transport not supported: {0}")]
    NotSupported(String),

    /// An internal invariant was violated.
    #[error("internal error: {0}")]
    Internal(String),
}

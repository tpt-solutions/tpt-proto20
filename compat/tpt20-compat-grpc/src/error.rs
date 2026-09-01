//! Error types for the gRPC compatibility adapter.

use thiserror::Error;

/// Errors that can occur during gRPC compatibility operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GrpcError {
    /// An invalid gRPC status code was encountered.
    #[error("invalid gRPC status code: {0}")]
    InvalidStatus(i32),

    /// An invalid grpc-timeout header value was encountered.
    #[error("invalid grpc-timeout value: {0}")]
    InvalidTimeout(String),

    /// A malformed gRPC message frame was encountered.
    #[error("invalid gRPC frame: {0}")]
    InvalidFrame(String),

    /// A health check protocol error occurred.
    #[error("health check error: {0}")]
    HealthCheck(String),

    /// A reflection protocol error occurred.
    #[error("reflection error: {0}")]
    Reflection(String),

    /// A transport-level error occurred.
    #[error("transport error: {0}")]
    Transport(String),

    /// A metadata translation error occurred.
    #[error("metadata error: {0}")]
    Metadata(String),

    /// An HTTP error occurred.
    #[error("http error: {0}")]
    Http(String),

    /// The requested operation is not supported.
    #[error("not supported: {0}")]
    NotSupported(String),
}

impl From<tpt20_rpc::metadata::MetadataError> for GrpcError {
    fn from(err: tpt20_rpc::metadata::MetadataError) -> Self {
        GrpcError::Metadata(err.to_string())
    }
}

impl From<tpt20_transport::TransportError> for GrpcError {
    fn from(err: tpt20_transport::TransportError) -> Self {
        GrpcError::Transport(err.to_string())
    }
}

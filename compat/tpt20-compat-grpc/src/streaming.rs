//! Streaming mode mapping between tpt20 and gRPC (spec §10.3).
//!
//! gRPC streaming types correspond directly to tpt20 streaming types. This
//! module provides explicit conversions.

use tpt20_transport::StreamingType;

/// gRPC streaming mode.
///
/// Maps 1:1 to tpt20 [`StreamingType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrpcStreamingType {
    /// Unary call: one request, one response.
    Unary,
    /// Server streaming: one request, many responses.
    ServerStream,
    /// Client streaming: many requests, one response.
    ClientStream,
    /// Bidirectional streaming: many requests, many responses.
    Bidi,
}

/// Placeholder for streaming semantics documentation.
pub struct StreamingSemantics;

impl From<GrpcStreamingType> for StreamingType {
    fn from(streaming: GrpcStreamingType) -> Self {
        match streaming {
            GrpcStreamingType::Unary => StreamingType::Unary,
            GrpcStreamingType::ServerStream => StreamingType::ServerStream,
            GrpcStreamingType::ClientStream => StreamingType::ClientStream,
            GrpcStreamingType::Bidi => StreamingType::Bidi,
        }
    }
}

impl From<StreamingType> for GrpcStreamingType {
    fn from(streaming: StreamingType) -> Self {
        match streaming {
            StreamingType::Unary => GrpcStreamingType::Unary,
            StreamingType::ServerStream => GrpcStreamingType::ServerStream,
            StreamingType::ClientStream => GrpcStreamingType::ClientStream,
            StreamingType::Bidi => GrpcStreamingType::Bidi,
        }
    }
}

/// Converts a tpt20 [`StreamingType`] to a gRPC streaming type.
pub fn to_grpc_streaming(streaming: StreamingType) -> GrpcStreamingType {
    streaming.into()
}

/// Converts a gRPC streaming type to a tpt20 [`StreamingType`].
pub fn from_grpc_streaming(streaming: GrpcStreamingType) -> StreamingType {
    streaming.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_roundtrip() {
        for original in [
            StreamingType::Unary,
            StreamingType::ServerStream,
            StreamingType::ClientStream,
            StreamingType::Bidi,
        ] {
            let grpc = to_grpc_streaming(original);
            let back = from_grpc_streaming(grpc);
            assert_eq!(back, original);
        }
    }
}

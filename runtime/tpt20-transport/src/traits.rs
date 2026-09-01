//! Transport traits: the transport-agnostic interface between RPC and the
//! underlying transport implementation (spec §17).

use crate::error::TransportError;
use crate::metadata::Metadata;
use async_trait::async_trait;
use futures::{Sink, Stream};
use std::pin::Pin;

/// The streaming type of an RPC call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingType {
    /// Unary call: one request message, one response message.
    Unary,
    /// Server streaming: one request message, many response messages.
    ServerStream,
    /// Client streaming: many request messages, one response message.
    ClientStream,
    /// Bidirectional streaming: many request messages, many response messages.
    Bidi,
}

/// An item received from a response stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamItem {
    /// A message payload.
    Message(Vec<u8>),
    /// Trailers carrying final status and trailing metadata.
    Trailer(Metadata),
}

/// A single RPC call, providing a sink for requests and a stream for responses.
pub struct Call {
    /// Sink for sending request messages. Close with `Sink::close` when done.
    pub sink: Pin<Box<dyn Sink<Vec<u8>, Error = TransportError> + Send + Sync + Unpin>>,
    /// Stream of response messages and trailers.
    pub stream:
        Pin<Box<dyn Stream<Item = Result<StreamItem, TransportError>> + Send + Sync + Unpin>>,
}

impl Call {
    /// Creates a new call from a sink and stream.
    pub fn new(
        sink: Pin<Box<dyn Sink<Vec<u8>, Error = TransportError> + Send + Sync + Unpin>>,
        stream: Pin<Box<dyn Stream<Item = Result<StreamItem, TransportError>> + Send + Sync + Unpin>>,
    ) -> Self {
        Call { sink, stream }
    }
}

/// The transport trait: transport-agnostic interface for initiating RPC calls.
///
/// Implementations provide the underlying communication (in-process, HTTP/2,
/// QUIC, custom). The RPC layer uses this trait without depending on any
/// specific transport technology.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Starts a new RPC call.
    ///
    /// Returns a [`Call`] containing a sink for request messages and a stream
    /// for response messages/trailers.
    async fn start_call(
        &self,
        method: &str,
        request: Vec<u8>,
        metadata: &Metadata,
        streaming_type: StreamingType,
    ) -> Result<Call, TransportError>;
}

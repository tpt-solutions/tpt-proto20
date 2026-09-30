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

/// Stream of response messages and trailers.
pub type ResponseStream =
    Pin<Box<dyn Stream<Item = Result<StreamItem, TransportError>> + Send + Sync + Unpin>>;

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
        stream: Pin<
            Box<dyn Stream<Item = Result<StreamItem, TransportError>> + Send + Sync + Unpin>,
        >,
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

/// Stream of further request messages received by a server.
pub type RequestStream = Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>;

/// The sending half of a server-side call: response messages and trailers.
#[async_trait]
pub trait CallSender: Send + Sync {
    /// Sends one response message to the client.
    async fn send_message(&self, payload: Vec<u8>) -> Result<(), TransportError>;

    /// Sends the trailing metadata and ends the response. Nothing may be
    /// sent afterwards.
    async fn send_trailers(&mut self, trailers: Metadata) -> Result<(), TransportError>;

    /// A future that completes when the peer is gone (the client dropped the
    /// call, reset the stream, or the connection closed). It also completes
    /// once the call has ended normally, so it must only be used to cancel
    /// work that is still running. The default never completes.
    fn closed_signal(&self) -> futures::future::BoxFuture<'static, ()> {
        Box::pin(futures::future::pending())
    }
}

/// A server-side call split into independently usable halves, so a handler
/// can read the request stream while writing responses (bidi streaming).
pub struct IncomingCallParts {
    /// The RPC method path (e.g. `pkg.Service/Method`).
    pub method: String,
    /// Request metadata.
    pub metadata: Metadata,
    /// The initial request message.
    pub request: Vec<u8>,
    /// Further request messages (client streaming / bidi).
    pub incoming: RequestStream,
    /// Response half.
    pub sender: Box<dyn CallSender>,
}

/// A call received by a transport's server, in transport-neutral form.
pub trait IncomingCall: Send {
    /// Splits the call into its request and response halves.
    fn into_parts(self) -> IncomingCallParts;
}

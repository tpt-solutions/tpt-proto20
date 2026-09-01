//! gRPC-compatible HTTP/2 client (spec §10.3).
//!
//! Provides a client that initiates gRPC HTTP/2 calls by translating tpt20
//! transport calls into gRPC HTTP/2 requests.
//!
//! ## Usage
//!
//! ```
//! # use tpt20_compat_grpc::client::GrpcClient;
//! # use tpt20_transport::{Endpoint, StreamingType, InProcessTransport};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let endpoint = Endpoint::new("in-process://test");
//! // let transport = InProcessTransport::new(endpoint);
//! // let client = GrpcClient::new(transport);
//! // let response = client.call(
//! //     "user.v1.UserService/GetUser",
//! //     StreamingType::Unary,
//! //     request_bytes,
//! // ).await?;
//! # Ok(())
//! # }
//! ```

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{Sink, SinkExt, Stream, StreamExt};
use tpt20_transport::{Call, StreamingType, Transport};
use tpt20_transport::traits::StreamItem;

use crate::GrpcError;

/// A gRPC-compatible client.
///
/// Wraps a tpt20 [`Transport`] and translates gRPC concepts to/from the
/// underlying transport.
pub struct GrpcClient {
    transport: std::sync::Arc<dyn Transport>,
}

impl GrpcClient {
    /// Creates a new gRPC client backed by the given transport.
    pub fn new(transport: impl Transport + 'static) -> Self {
        GrpcClient {
            transport: std::sync::Arc::new(transport),
        }
    }

    /// Creates a new gRPC client from a shared transport reference.
    pub fn from_shared(transport: std::sync::Arc<dyn Transport>) -> Self {
        GrpcClient { transport }
    }

    /// Initiates a gRPC call.
    pub async fn call(
        &self,
        method: &str,
        streaming_type: StreamingType,
        request: Vec<u8>,
    ) -> Result<GrpcCall, GrpcError> {
        let metadata = tpt20_transport::Metadata::new();
        let call = self
            .transport
            .start_call(method, request, &metadata, streaming_type)
            .await?;
        Ok(GrpcCall::new(call))
    }
}

/// A handle to an ongoing gRPC call.
pub struct GrpcCall {
    sink: Pin<Box<dyn Sink<Vec<u8>, Error = GrpcError> + Send + Sync + Unpin>>,
    stream: Pin<Box<dyn Stream<Item = Result<GrpcResponse, GrpcError>> + Send + Sync + Unpin>>,
}

impl GrpcCall {
    fn new(call: Call) -> Self {
        let sink = GrpcSink::new(call.sink);
        let stream = GrpcStream::new(call.stream);
        GrpcCall {
            sink: Box::pin(sink),
            stream: Box::pin(stream),
        }
    }

    /// Sends a request message.
    pub async fn send(&mut self, payload: Vec<u8>) -> Result<(), GrpcError> {
        self.sink.send(payload).await
    }

    /// Receives the next response message or trailer.
    pub async fn next(&mut self) -> Option<Result<GrpcResponse, GrpcError>> {
        self.stream.next().await
    }

    /// Closes the request stream.
    pub async fn close(mut self) -> Result<(), GrpcError> {
        futures::SinkExt::close(&mut self.sink).await?;
        Ok(())
    }
}

impl futures::Stream for GrpcCall {
    type Item = Result<GrpcResponse, GrpcError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream).poll_next(cx)
    }
}

/// A sink adapter that converts [`TransportError`] to [`GrpcError`].
struct GrpcSink {
    inner: Pin<Box<dyn Sink<Vec<u8>, Error = tpt20_transport::TransportError> + Send + Sync + Unpin>>,
}

impl GrpcSink {
    fn new(
        inner: Pin<Box<dyn Sink<Vec<u8>, Error = tpt20_transport::TransportError> + Send + Sync + Unpin>>,
    ) -> Self {
        GrpcSink { inner }
    }
}

impl Sink<Vec<u8>> for GrpcSink {
    type Error = GrpcError;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_ready(cx)
            .map_err(GrpcError::from)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner)
            .start_send(item)
            .map_err(GrpcError::from)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_flush(cx)
            .map_err(GrpcError::from)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_close(cx)
            .map_err(GrpcError::from)
    }
}

/// A stream adapter that converts [`TransportError`] to [`GrpcError`].
struct GrpcStream {
    inner: Pin<Box<dyn Stream<Item = Result<StreamItem, tpt20_transport::TransportError>> + Send + Sync + Unpin>>,
}

impl GrpcStream {
    fn new(
        inner: Pin<Box<dyn Stream<Item = Result<StreamItem, tpt20_transport::TransportError>> + Send + Sync + Unpin>>,
    ) -> Self {
        GrpcStream { inner }
    }
}

impl Stream for GrpcStream {
    type Item = Result<GrpcResponse, GrpcError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(item))) => match item {
                StreamItem::Message(payload) => {
                    Poll::Ready(Some(Ok(GrpcResponse::Message(payload))))
                }
                StreamItem::Trailer(trailers) => {
                    Poll::Ready(Some(Ok(GrpcResponse::Trailers {
                        status: tpt20_rpc::Status::Ok,
                        message: String::new(),
                        metadata: trailers,
                    })))
                }
            },
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(GrpcError::from(e)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A single item received from a gRPC response stream.
#[derive(Debug, Clone)]
pub enum GrpcResponse {
    /// A message payload.
    Message(Vec<u8>),
    /// Final trailers containing status and metadata.
    Trailers {
        /// The gRPC status code.
        status: tpt20_rpc::Status,
        /// Optional status message.
        message: String,
        /// Trailing metadata.
        metadata: tpt20_transport::Metadata,
    },
}

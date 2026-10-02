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
use tpt20_transport::traits::StreamItem;
use tpt20_transport::{Call, StreamingType, Transport};

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
    inner:
        Pin<Box<dyn Sink<Vec<u8>, Error = tpt20_transport::TransportError> + Send + Sync + Unpin>>,
}

impl GrpcSink {
    fn new(
        inner: Pin<
            Box<dyn Sink<Vec<u8>, Error = tpt20_transport::TransportError> + Send + Sync + Unpin>,
        >,
    ) -> Self {
        GrpcSink { inner }
    }
}

impl Sink<Vec<u8>> for GrpcSink {
    type Error = GrpcError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_ready(cx)
            .map_err(GrpcError::from)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner)
            .start_send(item)
            .map_err(GrpcError::from)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_flush(cx)
            .map_err(GrpcError::from)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_close(cx)
            .map_err(GrpcError::from)
    }
}

/// A stream adapter that converts [`TransportError`] to [`GrpcError`].
struct GrpcStream {
    inner: Pin<
        Box<
            dyn Stream<Item = Result<StreamItem, tpt20_transport::TransportError>>
                + Send
                + Sync
                + Unpin,
        >,
    >,
}

impl GrpcStream {
    fn new(
        inner: Pin<
            Box<
                dyn Stream<Item = Result<StreamItem, tpt20_transport::TransportError>>
                    + Send
                    + Sync
                    + Unpin,
            >,
        >,
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
                StreamItem::Trailer(trailers) => Poll::Ready(Some(decode_trailers(trailers))),
            },
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(GrpcError::from(e)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Extracts `grpc-status` / `grpc-message` from response trailers.
///
/// Per the gRPC protocol a response that ends without `grpc-status` is a
/// failure ([`Status::Unknown`](tpt20_rpc::Status::Unknown)), never success.
/// The two protocol keys are removed from the returned metadata.
fn decode_trailers(mut trailers: tpt20_transport::Metadata) -> Result<GrpcResponse, GrpcError> {
    let first =
        |md: &tpt20_transport::Metadata, key: &str| md.get(key).and_then(|v| v.first()).cloned();
    let status = match first(&trailers, "grpc-status") {
        Some(code) => {
            let code: i32 = code
                .trim()
                .parse()
                .map_err(|_| GrpcError::Metadata(format!("invalid grpc-status `{code}`")))?;
            crate::status::from_grpc_status(code)?
        }
        None => tpt20_rpc::Status::Unknown,
    };
    let message = first(&trailers, "grpc-message")
        .map(|m| percent_decode(&m))
        .unwrap_or_else(|| {
            if first(&trailers, "grpc-status").is_none() {
                "missing grpc-status trailer".to_string()
            } else {
                String::new()
            }
        });
    trailers.remove("grpc-status");
    trailers.remove("grpc-message");
    Ok(GrpcResponse::Trailers {
        status,
        message,
        metadata: trailers,
    })
}

/// Decodes the percent-encoding gRPC applies to `grpc-message`.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
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

#[cfg(test)]
mod tests {
    use super::*;
    use tpt20_rpc::Status;
    use tpt20_transport::{InProcessServer, Metadata};

    fn trailers(pairs: &[(&str, &str)]) -> Metadata {
        let mut md = Metadata::new();
        for (k, v) in pairs {
            md.insert(*k, *v);
        }
        md
    }

    fn unpack(r: GrpcResponse) -> (Status, String, Metadata) {
        match r {
            GrpcResponse::Trailers {
                status,
                message,
                metadata,
            } => (status, message, metadata),
            other => panic!("expected trailers, got {other:?}"),
        }
    }

    #[test]
    fn trailers_carry_status_message_and_metadata() {
        let (status, message, md) = unpack(
            decode_trailers(trailers(&[
                ("grpc-status", "5"),
                ("grpc-message", "no%20such%20user%3A%20%C3%A9"),
                ("x-extra", "1"),
            ]))
            .unwrap(),
        );
        assert_eq!(status, Status::NotFound);
        assert_eq!(message, "no such user: é");
        assert!(md.get("grpc-status").is_none() && md.get("grpc-message").is_none());
        assert_eq!(md.get("x-extra"), Some(&["1".to_string()][..]));
    }

    #[test]
    fn ok_status_and_missing_status() {
        let (status, message, _) =
            unpack(decode_trailers(trailers(&[("grpc-status", "0")])).unwrap());
        assert_eq!((status, message.as_str()), (Status::Ok, ""));

        // No grpc-status at all must not be reported as success.
        let (status, message, _) = unpack(decode_trailers(Metadata::new()).unwrap());
        assert_eq!(status, Status::Unknown);
        assert!(message.contains("missing grpc-status"));
    }

    #[test]
    fn invalid_status_codes_are_errors() {
        assert!(decode_trailers(trailers(&[("grpc-status", "abc")])).is_err());
        assert!(decode_trailers(trailers(&[("grpc-status", "99")])).is_err());
    }

    #[test]
    fn percent_decode_tolerates_malformed_escapes() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("%41%42"), "AB");
    }

    #[tokio::test]
    async fn failed_call_is_reported_as_failure_end_to_end() {
        let (server, mut requests) = InProcessServer::bind(4);
        tokio::spawn(async move {
            while let Some(mut req) = requests.recv().await {
                let _ = req.send_message(b"partial".to_vec()).await;
                let _ = req
                    .send_trailers(trailers(&[("grpc-status", "13"), ("grpc-message", "boom")]))
                    .await;
            }
        });
        let client = GrpcClient::new(server.transport());
        let mut call = client
            .call("svc/Method", StreamingType::Unary, b"req".to_vec())
            .await
            .unwrap();
        assert!(matches!(
            call.next().await.unwrap().unwrap(),
            GrpcResponse::Message(m) if m == b"partial"
        ));
        let (status, message, _) = unpack(call.next().await.unwrap().unwrap());
        assert_eq!(status, Status::Internal);
        assert_eq!(message, "boom");
    }
}

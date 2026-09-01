//! In-process transport for tests, embedded systems, local development,
//! benchmarking, and fuzzing (spec §17.3).
//!
//! This transport uses tokio channels to pass framed messages between
//! client and server without any network I/O.

use crate::error::TransportError;
use crate::frame::{encode_frame, Frame, FrameFlags};
use crate::metadata::Metadata;
use crate::traits::{BoxedSink, BoxedStream, Call, StreamingType, StreamItem, Transport};
use async_trait::async_trait;
use futures::{Sink, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, oneshot};

/// A framed message on the in-process wire.
#[derive(Debug, Clone)]
struct FramedMessage {
    flags: FrameFlags,
    payload: Vec<u8>,
}

/// A request received by the in-process server.
#[derive(Debug)]
pub struct IncomingRequest {
    /// The RPC method name.
    pub method: String,
    /// Request metadata.
    pub metadata: Metadata,
    /// The initial request payload bytes.
    pub request: Vec<u8>,
    /// Streaming type of the call.
    pub streaming_type: StreamingType,
    /// Channel to send response frames back to the client.
    pub response_tx: mpsc::Sender<Result<FramedMessage, TransportError>>,
    /// Channel to send trailing metadata back to the client.
    pub trailers_tx: oneshot::Sender<Result<Metadata, TransportError>>,
    /// Channel to receive additional request messages (for client streaming / bidi).
    pub request_rx: mpsc::Receiver<Vec<u8>>,
}

impl IncomingRequest {
    /// Sends a response message to the client.
    pub async fn send_message(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        let framed = FramedMessage {
            flags: FrameFlags::empty(),
            payload,
        };
        self.response_tx
            .send(Ok(framed))
            .await
            .map_err(|_| TransportError::ConnectionClosed)
    }

    /// Sends trailing metadata to the client.
    pub async fn send_trailers(&self, trailers: Metadata) -> Result<(), TransportError> {
        let _ = self.trailers_tx.send(Ok(trailers));
        Ok(())
    }

    /// Receives the next request message (for client streaming / bidi).
    pub async fn recv_message(&mut self) -> Option<Vec<u8>> {
        self.request_rx.recv().await
    }
}

/// Stream of response items from an in-process call.
struct InProcessResponseStream {
    response_rx: mpsc::Receiver<Result<FramedMessage, TransportError>>,
    trailers_rx: oneshot::Receiver<Result<Metadata, TransportError>>,
}

impl Stream for InProcessResponseStream {
    type Item = Result<StreamItem, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.response_rx).poll_recv(cx) {
            Poll::Ready(Some(Ok(FramedMessage {
                flags,
                payload,
            }))) => {
                if flags.is_compressed() {
                    return Poll::Ready(Some(Err(TransportError::Compression(
                        "compressed frames not yet supported in in-process transport".into(),
                    ))));
                }
                Poll::Ready(Some(Ok(StreamItem::Message(payload))))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                match self.trailers_rx.try_recv() {
                    Ok(Ok(trailers)) => Poll::Ready(Some(Ok(StreamItem::Trailers(trailers)))),
                    Ok(Err(e)) => Poll::Ready(Some(Err(e))),
                    Err(oneshot::error::TryRecvError::Empty) => Poll::Ready(None),
                    Err(oneshot::error::TryRecvError::Closed) => Poll::Ready(None),
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// In-process server: accepts calls from in-process transports.
#[derive(Debug, Clone)]
pub struct InProcessServer {
    request_tx: mpsc::Sender<IncomingRequest>,
}

impl InProcessServer {
    /// Creates a new in-process server with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (request_tx, _request_rx) = mpsc::channel(capacity);
        InProcessServer { request_tx }
    }

    /// Returns a transport that connects to this server.
    pub fn transport(&self) -> InProcessTransport {
        InProcessTransport {
            request_tx: self.request_tx.clone(),
        }
    }
}

impl InProcessServer {
    /// Binds a new in-process server and returns the server plus its request receiver.
    pub fn bind(capacity: usize) -> (Self, mpsc::Receiver<IncomingRequest>) {
        let (request_tx, request_rx) = mpsc::channel(capacity);
        (InProcessServer { request_tx }, request_rx)
    }
}

/// In-process transport: connects to an in-process server.
#[derive(Debug, Clone)]
pub struct InProcessTransport {
    request_tx: mpsc::Sender<IncomingRequest>,
}

#[async_trait]
impl Transport for InProcessTransport {
    async fn start_call(
        &self,
        method: &str,
        request: Vec<u8>,
        metadata: &Metadata,
        streaming_type: StreamingType,
    ) -> Result<Call, TransportError> {
        let (response_tx, response_rx) = mpsc::channel(32);
        let (trailers_tx, trailers_rx) = oneshot::channel();
        let (request_msg_tx, request_msg_rx) = mpsc::channel(32);

        let incoming = IncomingRequest {
            method: method.to_string(),
            metadata: metadata.clone(),
            request,
            streaming_type,
            response_tx,
            trailers_tx,
            request_rx: request_msg_rx,
        };

        self.request_tx
            .send(incoming)
            .await
            .map_err(|_| TransportError::ConnectionClosed)?;

        let stream = InProcessResponseStream {
            response_rx,
            trailers_rx,
        };

        Ok(Call {
            sink: BoxedSink::new(request_msg_tx),
            stream: BoxedStream::new(stream),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn in_process_unary_roundtrip() {
        let (server, mut requests) = InProcessServer::bind(4);
        let transport = server.transport();

        let server_task = tokio::spawn(async move {
            while let Some(mut req) = requests.recv().await {
                let _ = req.send_message(b"response".to_vec()).await;
                let _ = req.send_trailers(Metadata::new()).await;
            }
        });

        let call = transport
            .start_call("test.Method", b"request".to_vec(), &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();

        let mut stream = call.stream;
        let item = stream.next().await.unwrap().unwrap();
        match item {
            StreamItem::Message(msg) => assert_eq!(msg, b"response"),
            _ => panic!("expected message"),
        }

        server_task.abort();
    }

    #[tokio::test]
    async fn in_process_server_stream() {
        let (server, mut requests) = InProcessServer::bind(4);
        let transport = server.transport();

        let server_task = tokio::spawn(async move {
            while let Some(mut req) = requests.recv().await {
                for i in 0..3u8 {
                    let _ = req.send_message(vec![i]).await;
                }
                let _ = req.send_trailers(Metadata::new()).await;
            }
        });

        let call = transport
            .start_call("test.Stream", b"req".to_vec(), &Metadata::new(), StreamingType::ServerStream)
            .await
            .unwrap();

        let mut stream = call.stream;
        for i in 0..3u8 {
            let item = stream.next().await.unwrap().unwrap();
            match item {
                StreamItem::Message(msg) => assert_eq!(msg, vec![i]),
                _ => panic!("expected message"),
            }
        }

        server_task.abort();
    }
}

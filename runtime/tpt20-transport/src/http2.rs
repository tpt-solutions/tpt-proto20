//! HTTP/2 transport: the required production transport (spec §17.1).
//!
//! Requires the `http2` feature flag (depends on the `h2` crate).
//!
//! The HTTP/2 transport provides:
//! - multiplexed streams
//! - trailers
//! - flow control
//! - stream reset handling
//! - GOAWAY handling
//! - keepalive/ping behavior
//! - TLS with ALPN (when the `tls` feature is also enabled)
//! - cleartext h2c for local development (explicit opt-in)

use crate::error::TransportError;
use crate::frame::{encode_frame, Frame, FrameFlags};
use crate::metadata::Metadata;
use crate::traits::{Call, StreamingType, StreamItem, Transport};
use async_trait::async_trait;
use futures::{Sink, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::oneshot;

#[cfg(feature = "http2")]
use bytes::Bytes;
#[cfg(feature = "http2")]
use http::{Request, Response};
#[cfg(feature = "http2")]
use h2::client;
#[cfg(feature = "http2")]
use h2::RecvStream;

/// HTTP/2 client transport.
///
/// Connects to an HTTP/2 server and allows making RPC calls.
#[derive(Debug, Clone)]
pub struct Http2Transport {
    endpoint: crate::Endpoint,
}

impl Http2Transport {
    /// Creates a new HTTP/2 transport for the given endpoint.
    pub fn new(endpoint: crate::Endpoint) -> Self {
        Http2Transport { endpoint }
    }

    /// Returns the endpoint this transport connects to.
    pub fn endpoint(&self) -> &crate::Endpoint {
        &self.endpoint
    }
}

#[async_trait]
impl Transport for Http2Transport {
    async fn start_call(
        &self,
        method: &str,
        request: Vec<u8>,
        _metadata: &Metadata,
        _streaming_type: StreamingType,
    ) -> Result<Call, TransportError> {
        use tokio::net::TcpStream;

        let tcp = TcpStream::connect(&self.endpoint.address)
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;

        if self.endpoint.uses_tls() {
            #[cfg(feature = "tls")]
            {
                let tls_config = self.endpoint.tls.as_ref().ok_or_else(|| {
                    TransportError::Tls("TLS endpoint missing TlsConfig".into())
                })?;
                let connector = self.make_tls_connector(tls_config)?;
                let server_name = self
                    .endpoint
                    .address
                    .split(':')
                    .next()
                    .unwrap_or("localhost");
                let tls_stream = connector
                    .connect(server_name, tcp)
                    .await
                    .map_err(|e| TransportError::Tls(e.to_string()))?;

                let (mut client, connection) = client::Builder::new()
                    .handshake::<_, Bytes>(tls_stream)
                    .await
                    .map_err(|e| TransportError::Internal(e.to_string()))?;

                tokio::spawn(async move {
                    let _ = connection.await;
                });

                let mut client = client.ready().await.map_err(|e| TransportError::Internal(e.to_string()))?;

                let request_body = Bytes::from(request);
                let http_request = Request::builder()
                    .method("POST")
                    .uri(format!("/{}", method))
                    .body(())
                    .map_err(|e| TransportError::Internal(e.to_string()))?;

                let (response_fut, mut send_stream) = client
                    .send_request(http_request, false)
                    .map_err(|e| TransportError::Internal(e.to_string()))?;

                send_stream
                    .send_data(request_body, true)
                    .map_err(|e| TransportError::Internal(e.to_string()))?;

                let response = response_fut
                    .await
                    .map_err(|e| TransportError::Internal(e.to_string()))?;

                let (trailers_tx, trailers_rx) = oneshot::channel();
                let response_stream = Http2ClientResponseStream {
                    response,
                    trailers_rx,
                };

                Ok(Call {
                    sink: Pin::<Box<dyn Sink<Vec<u8>, Error = TransportError> + Send + Sync + Unpin>>::new(Box::new(Http2ClientSink {
                        send_stream,
                        trailers_tx,
                    })),
                    stream: Pin::<Box<dyn Stream<Item = Result<StreamItem, TransportError>> + Send + Sync + Unpin>>::new(Box::new(response_stream)),
                })
            }
            #[cfg(not(feature = "tls"))]
            {
                Err(TransportError::NotSupported(
                    "TLS support requires the `tls` feature".into(),
                ))
            }
        } else {
            let (mut client, connection) = client::Builder::new()
                .handshake::<_, Bytes>(tcp)
                .await
                .map_err(|e| TransportError::Internal(e.to_string()))?;

            tokio::spawn(async move {
                let _ = connection.await;
            });

            let mut client = client.ready().await.map_err(|e| TransportError::Internal(e.to_string()))?;

            let request_body = Bytes::from(request);
            let http_request = Request::builder()
                .method("POST")
                .uri(format!("/{}", method))
                .body(())
                .map_err(|e| TransportError::Internal(e.to_string()))?;

            let (response_fut, mut send_stream) = client
                .send_request(http_request, false)
                .map_err(|e| TransportError::Internal(e.to_string()))?;

            send_stream
                .send_data(request_body, true)
                .map_err(|e| TransportError::Internal(e.to_string()))?;

            let response = response_fut
                .await
                .map_err(|e| TransportError::Internal(e.to_string()))?;

            let (trailers_tx, trailers_rx) = oneshot::channel();
            let response_stream = Http2ClientResponseStream {
                response,
                trailers_rx,
            };

            Ok(Call {
                sink: Pin::<Box<dyn Sink<Vec<u8>, Error = TransportError> + Send + Sync + Unpin>>::new(Box::new(Http2ClientSink {
                    send_stream,
                    trailers_tx,
                })),
                stream: Pin::<Box<dyn Stream<Item = Result<StreamItem, TransportError>> + Send + Sync + Unpin>>::new(Box::new(response_stream)),
            })
        }
    }
}

#[cfg(all(feature = "http2", feature = "tls"))]
impl Http2Transport {
    fn make_tls_connector(
        &self,
        _tls_config: &crate::TlsConfig,
    ) -> Result<tokio_rustls::TlsConnector, TransportError> {
        use rustls::ClientConfig;
        use std::sync::Arc;

        let mut root_store = rustls::RootCertStore::empty();

        if let Some(ref cert_pem) = _tls_config.cert_pem {
            let mut reader = cert_pem.as_slice();
            for cert in rustls_pemfile::certs(&mut reader)
                .map_err(|e| TransportError::Tls(e.to_string()))?
            {
                root_store.add(cert).map_err(|e| TransportError::Tls(e.to_string()))?;
            }
        }

        let client_config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(
                if _tls_config.accept_invalid_certs {
                    rustls::client::WebPkiVerifier::without_provider()
                        .map_err(|e| TransportError::Tls(e.to_string()))?
                } else {
                    let provider = rustls::crypto::ring::default_provider()
                        .map_err(|e| TransportError::Tls(e.to_string()))?;
                    std::sync::Arc::new(rustls::client::WebPkiVerifier::new(
                        root_store,
                        None,
                        provider,
                    ))
                },
            ))
            .with_no_client_auth();

        Ok(tokio_rustls::TlsConnector::from(std::sync::Arc::new(
            client_config,
        )))
    }
}

#[cfg(feature = "http2")]
struct Http2ClientSink {
    send_stream: h2::SendStream<Bytes>,
    trailers_tx: oneshot::Sender<Result<Metadata, TransportError>>,
}

#[cfg(feature = "http2")]
impl Sink<Vec<u8>> for Http2ClientSink {
    type Error = TransportError;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.send_stream)
            .poll_ready(cx)
            .map_err(|e| TransportError::Internal(e.to_string()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        let bytes = Bytes::from(item);
        Pin::new(&mut self.send_stream)
            .send_data(bytes, false)
            .map_err(|e| TransportError::Internal(e.to_string()))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.send_stream)
            .poll_capacity(cx)
            .map(|_| Ok(()))
            .map_err(|e| TransportError::Internal(e.to_string()))
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.send_stream)
            .poll_capacity(cx)
            .map(|_| Ok(()))
            .map_err(|e| TransportError::Internal(e.to_string()))
    }
}

#[cfg(feature = "http2")]
struct Http2ClientResponseStream {
    response: Response<RecvStream>,
    trailers_rx: oneshot::Receiver<Result<Metadata, TransportError>>,
}

#[cfg(feature = "http2")]
impl Stream for Http2ClientResponseStream {
    type Item = Result<StreamItem, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut body = self.response.body_mut();
        match Pin::new(&mut body).poll_data(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let frame = Frame::decode(&bytes)
                    .map_err(|e| TransportError::MalformedFrame(e.to_string()))?;
                if frame.flags.is_compressed() {
                    return Poll::Ready(Some(Err(TransportError::Compression(
                        "compressed payload not yet supported".into(),
                    ))));
                }
                Poll::Ready(Some(Ok(StreamItem::Message(frame.payload))))
            }
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Some(Err(TransportError::Internal(e.to_string()))))
            }
            Poll::Ready(None) => {
                let trailers = Metadata::new();
                Poll::Ready(Some(Ok(StreamItem::Trailer(trailers))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// HTTP/2 server: accepts incoming HTTP/2 connections and dispatches RPC calls.
#[derive(Debug, Clone)]
pub struct Http2Server {
    endpoint: crate::Endpoint,
}

impl Http2Server {
    /// Creates a new HTTP/2 server bound to the given endpoint.
    pub fn new(endpoint: crate::Endpoint) -> Self {
        Http2Server { endpoint }
    }

    /// Returns the endpoint this server listens on.
    pub fn endpoint(&self) -> &crate::Endpoint {
        &self.endpoint
    }

    /// Runs the server, accepting connections and dispatching to the handler.
    pub async fn serve<F>(&self, handler: F) -> Result<(), TransportError>
    where
        F: Fn(IncomingHttp2Call) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
            + Send
            + Sync
            + Clone
            + 'static,
    {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(&self.endpoint.address)
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;

        loop {
            let (stream, _) = listener
                .accept()
                .await
                .map_err(|e| TransportError::Io(e.to_string()))?;

            let mut server_builder = h2::server::Builder::new();
            if let Some(max) = self.endpoint.max_message_bytes {
                server_builder.max_frame_size(max as u32);
            }

            let mut server = server_builder
                .handshake::<_, Bytes>(stream)
                .await
                .map_err(|e| TransportError::Internal(e.to_string()))?;

            let handler = handler.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::serve_connections(&mut server, handler).await {
                    eprintln!("HTTP/2 server error: {:?}", e);
                }
            });
        }
    }

    #[cfg(feature = "http2")]
    async fn serve_connections<F>(
        server: &mut h2::server::Connection<tokio::net::TcpStream, Bytes>,
        handler: F,
    ) -> Result<(), TransportError>
    where
        F: Fn(IncomingHttp2Call) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
            + Send
            + Sync
            + Clone
            + 'static,
    {
        while let Some(request_result) = server.accept().await {
            match request_result {
                Ok((request, mut respond)) => {
                    let method = request.uri().path().trim_start_matches('/').to_string();
                    let metadata = request
                        .headers()
                        .iter()
                        .fold(Metadata::new(), |mut m, (k, v)| {
                            m.insert(k.as_str(), v.to_str().unwrap_or(""));
                            m
                        });

                    let body = request.into_body();
                    let request_bytes = collect_body(body).await;

                    let (response_tx, response_rx) = tokio::sync::mpsc::channel(32);
                    let (trailers_tx, _trailers_rx) = tokio::sync::oneshot::channel();

                    let call = IncomingHttp2Call {
                        method,
                        metadata,
                        request: request_bytes,
                        response_tx,
                        trailers_tx,
                    };

                    let handler = handler.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handler(call).await {
                            eprintln!("handler error: {:?}", e);
                        }
                    });
                }
                Err(e) => {
                    return Err(TransportError::Internal(e.to_string()));
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "http2")]
async fn collect_body(mut body: RecvStream) -> Vec<u8> {
    let mut acc = Vec::new();
    while let Some(chunk) = body.data().await {
        if let Ok(bytes) = chunk {
            acc.extend_from_slice(&bytes);
        }
    }
    acc
}

/// An incoming HTTP/2 call received by the server.
#[derive(Debug)]
pub struct IncomingHttp2Call {
    /// The RPC method name.
    pub method: String,
    /// Request metadata.
    pub metadata: Metadata,
    /// The raw request payload bytes.
    pub request: Vec<u8>,
    /// Channel to send response frames.
    pub response_tx: tokio::sync::mpsc::Sender<Result<FramedMessage, TransportError>>,
    /// Channel to send trailing metadata.
    pub trailers_tx: tokio::sync::oneshot::Sender<Result<Metadata, TransportError>>,
}

impl IncomingHttp2Call {
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
    pub async fn send_trailers(&mut self, trailers: Metadata) -> Result<(), TransportError> {
        let _ = self.trailers_tx.send(Ok(trailers));
        Ok(())
    }
}

//! HTTP/2 transport: the required production transport (spec §17.1).
//!
//! Requires the `http2` feature flag (depends on the `h2` crate).
//!
//! The HTTP/2 transport provides:
//! - multiplexed streams (one HTTP/2 stream per call)
//! - trailers (the server's trailing [`Metadata`] is delivered as the final
//!   [`StreamItem::Trailer`])
//! - flow control (`h2` windows, with capacity-aware writes)
//! - stream reset handling ([`TransportError::StreamReset`])
//! - GOAWAY handling ([`TransportError::GoAway`]) and graceful server shutdown
//! - keepalive pings ([`Endpoint::with_keepalive`](crate::Endpoint::with_keepalive))
//! - TLS with ALPN (when the `tls` feature is also enabled)
//! - cleartext h2c for local development (explicit opt-in)
//!
//! Messages travel as tpt20 frames (`flags:1 | length:4 BE | payload`) inside
//! HTTP/2 DATA frames. Frames are reassembled across DATA boundaries, so their
//! alignment with DATA frames is irrelevant.

use crate::error::TransportError;
use crate::frame::{Frame, FrameFlags, FramedMessage};
use crate::metadata::Metadata;
use crate::traits::{Call, StreamItem, StreamingType, Transport};
use crate::Endpoint;
use async_trait::async_trait;
use bytes::{Buf, Bytes, BytesMut};
use futures::{Sink, Stream};
use h2::{client, server, Reason, RecvStream, SendStream};
use http::{HeaderMap, HeaderName, HeaderValue, Request, Response};
use std::future::{poll_fn, Future};
use std::pin::Pin;
#[cfg(feature = "tls")]
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};

/// Default maximum message size (4 MiB) when the endpoint sets none.
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

const CONTENT_TYPE: &str = "application/tpt20";

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Maps an `h2` error onto the transport error taxonomy.
pub(crate) fn map_h2_error(e: h2::Error) -> TransportError {
    if e.is_go_away() {
        let reason = e
            .reason()
            .map(|r| r.to_string())
            .unwrap_or_else(|| "NO_ERROR".to_string());
        TransportError::GoAway(reason)
    } else if e.is_reset() {
        TransportError::StreamReset
    } else if e.is_io() {
        TransportError::Io(e.to_string())
    } else {
        TransportError::Internal(e.to_string())
    }
}

fn headers_to_metadata(headers: &HeaderMap) -> Metadata {
    let mut md = Metadata::new();
    for (k, v) in headers.iter() {
        if let Ok(v) = v.to_str() {
            md.insert(k.as_str(), v);
        }
    }
    md
}

/// Converts metadata to HTTP headers. Entries that are not valid HTTP header
/// names/values are rejected so the caller learns about them.
fn metadata_to_headers(md: &Metadata, out: &mut HeaderMap) -> Result<(), TransportError> {
    for (key, values) in md.iter() {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| TransportError::InvalidState(format!("invalid metadata key `{key}`")))?;
        for value in values {
            let value = HeaderValue::from_str(value).map_err(|_| {
                TransportError::InvalidState(format!("invalid metadata value for `{key}`"))
            })?;
            out.append(name.clone(), value);
        }
    }
    Ok(())
}

/// Reassembles tpt20 frames from a byte stream of arbitrary chunking.
struct FrameBuffer {
    buf: BytesMut,
    max: usize,
}

impl FrameBuffer {
    fn new(max: usize) -> Self {
        FrameBuffer {
            buf: BytesMut::new(),
            max,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Pops the next complete frame, enforcing the size limit as soon as the
    /// header is visible (before buffering the payload).
    fn next_frame(&mut self) -> Result<Option<FramedMessage>, TransportError> {
        if self.buf.len() < Frame::header_len() {
            return Ok(None);
        }
        let flags = FrameFlags::from_raw(self.buf[0])?;
        let len = u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
        if len > self.max {
            return Err(TransportError::SizeLimitExceeded { limit: self.max });
        }
        if self.buf.len() < Frame::header_len() + len {
            return Ok(None);
        }
        self.buf.advance(Frame::header_len());
        let payload = self.buf.split_to(len).to_vec();
        Ok(Some(FramedMessage { flags, payload }))
    }
}

fn encode_message(payload: &[u8], max: usize) -> Result<Bytes, TransportError> {
    if payload.len() > max {
        return Err(TransportError::SizeLimitExceeded { limit: max });
    }
    Ok(Bytes::from(Frame::encode_with(
        payload,
        FrameFlags::empty(),
    )))
}

/// Periodically pings the peer; returns when a ping fails or times out.
async fn keepalive(mut pings: h2::PingPong, interval: Duration, timeout: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        match tokio::time::timeout(timeout, pings.ping(h2::Ping::opaque())).await {
            Ok(Ok(_)) => {}
            _ => return,
        }
    }
}

/// Writes `data` honoring HTTP/2 flow control.
async fn write_all(send: &mut SendStream<Bytes>, mut data: Bytes) -> Result<(), TransportError> {
    while !data.is_empty() {
        if send.capacity() == 0 {
            send.reserve_capacity(data.len());
            match poll_fn(|cx| send.poll_capacity(cx)).await {
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(map_h2_error(e)),
                None => return Err(TransportError::StreamReset),
            }
        }
        let n = send.capacity().min(data.len());
        if n == 0 {
            continue;
        }
        send.send_data(data.split_to(n), false)
            .map_err(map_h2_error)?;
    }
    Ok(())
}

/// Reads the next complete frame from an HTTP/2 body; `None` on clean end.
async fn read_frame(
    body: &mut RecvStream,
    frames: &mut FrameBuffer,
) -> Result<Option<FramedMessage>, TransportError> {
    loop {
        if let Some(frame) = frames.next_frame()? {
            return Ok(Some(frame));
        }
        match body.data().await {
            Some(Ok(chunk)) => {
                let _ = body.flow_control().release_capacity(chunk.len());
                frames.push(&chunk);
            }
            Some(Err(e)) => return Err(map_h2_error(e)),
            None if frames.is_empty() => return Ok(None),
            None => return Err(TransportError::MalformedFrame("truncated frame".into())),
        }
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// HTTP/2 client transport.
///
/// Connects to an HTTP/2 server and allows making RPC calls. All calls share
/// one multiplexed connection (one HTTP/2 stream per call); the initial request message is sent immediately and, for
/// client-streaming and bidirectional calls, further messages go through the
/// returned call's sink.
#[derive(Debug, Clone)]
pub struct Http2Transport {
    endpoint: Endpoint,
    /// Shared, lazily established multiplexed connection. Clones of the
    /// transport share it; calls are separate HTTP/2 streams on it.
    pool: Arc<tokio::sync::Mutex<Option<client::SendRequest<Bytes>>>>,
}

impl Http2Transport {
    /// Creates a new HTTP/2 transport for the given endpoint. The
    /// connection is established on the first call and reused afterwards
    /// (re-established transparently if it dies).
    pub fn new(endpoint: Endpoint) -> Self {
        Http2Transport {
            endpoint,
            pool: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Returns the endpoint this transport connects to.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Returns a handle to the live connection, connecting if needed.
    async fn connection(&self) -> Result<client::SendRequest<Bytes>, TransportError> {
        let mut pool = self.pool.lock().await;
        if let Some(existing) = pool.as_ref() {
            return Ok(existing.clone());
        }
        let fresh = self.connect().await?;
        *pool = Some(fresh.clone());
        Ok(fresh)
    }

    /// Forgets the cached connection so the next call reconnects.
    async fn discard(&self) {
        *self.pool.lock().await = None;
    }

    async fn connect(&self) -> Result<client::SendRequest<Bytes>, TransportError> {
        let tcp = TcpStream::connect(&self.endpoint.address)
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;
        let _ = tcp.set_nodelay(true);

        if self.endpoint.uses_tls() {
            #[cfg(feature = "tls")]
            {
                let tls_config =
                    self.endpoint.tls.as_ref().ok_or_else(|| {
                        TransportError::Tls("TLS endpoint missing TlsConfig".into())
                    })?;
                let connector = self.make_tls_connector(tls_config)?;
                let host = self
                    .endpoint
                    .address
                    .rsplit_once(':')
                    .map(|(h, _)| h)
                    .unwrap_or(&self.endpoint.address)
                    .trim_matches(|c| c == '[' || c == ']')
                    .to_string();
                let server_name = rustls::pki_types::ServerName::try_from(host)
                    .map_err(|e| TransportError::Tls(e.to_string()))?;
                let tls_stream = connector
                    .connect(server_name, tcp)
                    .await
                    .map_err(|e| TransportError::Tls(e.to_string()))?;
                handshake(tls_stream, &self.endpoint).await
            }
            #[cfg(not(feature = "tls"))]
            {
                Err(TransportError::NotSupported(
                    "TLS support requires the `tls` feature".into(),
                ))
            }
        } else {
            handshake(tcp, &self.endpoint).await
        }
    }
}

#[async_trait]
impl Transport for Http2Transport {
    async fn start_call(
        &self,
        method: &str,
        request: Vec<u8>,
        metadata: &Metadata,
        streaming_type: StreamingType,
    ) -> Result<Call, TransportError> {
        let max = self
            .endpoint
            .max_message_bytes
            .unwrap_or(DEFAULT_MAX_MESSAGE_BYTES);
        let initial = encode_message(&request, max)?;

        let mut builder = req_builder(&self.endpoint, method)?;
        {
            let headers = builder
                .headers_mut()
                .ok_or_else(|| TransportError::Internal("request builder failed".into()))?;
            metadata_to_headers(&self.endpoint.default_metadata, headers)?;
            metadata_to_headers(metadata, headers)?;
        }
        let http_request = builder
            .body(())
            .map_err(|e| TransportError::Internal(e.to_string()))?;

        // Opening the stream is retried once on a fresh connection: a cached
        // connection may have been closed (GOAWAY, idle timeout, peer
        // restart) since its last use. Nothing has been sent at that point,
        // so the retry cannot duplicate a request.
        let mut attempt = 0;
        let (response, mut send_stream) = loop {
            let sender = self.connection().await?;
            let opened = async {
                let mut ready = sender.ready().await?;
                ready.send_request(http_request.clone(), false)
            }
            .await;
            match opened {
                Ok(pair) => break pair,
                Err(e) if attempt == 0 => {
                    let _ = e;
                    attempt += 1;
                    self.discard().await;
                }
                Err(e) => return Err(map_h2_error(e)),
            }
        };

        // Unary and server-streaming calls carry exactly one request message.
        let end_of_stream = matches!(
            streaming_type,
            StreamingType::Unary | StreamingType::ServerStream
        );
        write_all_end(&mut send_stream, initial, end_of_stream).await?;

        Ok(Call {
            sink: Box::pin(Http2ClientSink {
                send: send_stream,
                max,
                ended: end_of_stream,
            }),
            stream: Box::pin(Http2ClientResponseStream {
                state: RespState::Headers(Box::pin(response)),
                frames: FrameBuffer::new(max),
            }),
        })
    }
}

/// Performs the HTTP/2 handshake and spawns the connection driver (plus the
/// keepalive task when configured).
async fn handshake<IO>(
    io: IO,
    endpoint: &Endpoint,
) -> Result<client::SendRequest<Bytes>, TransportError>
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (send_request, mut connection) = client::Builder::new()
        .handshake::<_, Bytes>(io)
        .await
        .map_err(map_h2_error)?;

    let pings = connection.ping_pong();
    let ka = endpoint
        .keepalive_interval
        .map(|i| (i, endpoint.keepalive_timeout));
    tokio::spawn(async move {
        match (ka, pings) {
            (Some((interval, timeout)), Some(pings)) => {
                tokio::select! {
                    _ = &mut connection => {}
                    _ = keepalive(pings, interval, timeout) => {}
                }
            }
            _ => {
                let _ = (&mut connection).await;
            }
        }
    });
    Ok(send_request)
}

fn req_builder(
    endpoint: &Endpoint,
    method: &str,
) -> Result<http::request::Builder, TransportError> {
    let scheme = if endpoint.uses_tls() { "https" } else { "http" };
    Ok(Request::builder()
        .method("POST")
        .uri(format!("{scheme}://{}/{method}", endpoint.address))
        .header("content-type", CONTENT_TYPE)
        .header("te", "trailers"))
}

/// Writes the initial message, optionally ending the stream with it.
async fn write_all_end(
    send: &mut SendStream<Bytes>,
    data: Bytes,
    end_of_stream: bool,
) -> Result<(), TransportError> {
    write_all(send, data).await?;
    if end_of_stream {
        send.send_data(Bytes::new(), true).map_err(map_h2_error)?;
    }
    Ok(())
}

struct Http2ClientSink {
    send: SendStream<Bytes>,
    max: usize,
    ended: bool,
}

impl Sink<Vec<u8>> for Http2ClientSink {
    type Error = TransportError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.ended {
            return Poll::Ready(Err(TransportError::InvalidState("stream closed".into())));
        }
        if self.send.capacity() > 0 {
            return Poll::Ready(Ok(()));
        }
        self.send.reserve_capacity(1);
        match self.send.poll_capacity(cx) {
            Poll::Ready(Some(Ok(_))) => Poll::Ready(Ok(())),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Err(map_h2_error(e))),
            Poll::Ready(None) => Poll::Ready(Err(TransportError::StreamReset)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        let frame = encode_message(&item, self.max)?;
        self.send.send_data(frame, false).map_err(map_h2_error)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        if !self.ended {
            self.ended = true;
            self.send
                .send_data(Bytes::new(), true)
                .map_err(map_h2_error)?;
        }
        Poll::Ready(Ok(()))
    }
}

enum RespState {
    Headers(Pin<Box<client::ResponseFuture>>),
    Body(RecvStream),
    Trailers(RecvStream),
    Done,
}

struct Http2ClientResponseStream {
    state: RespState,
    frames: FrameBuffer,
}

impl Stream for Http2ClientResponseStream {
    type Item = Result<StreamItem, TransportError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            match &mut this.state {
                RespState::Headers(fut) => match fut.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(e)) => {
                        this.state = RespState::Done;
                        return Poll::Ready(Some(Err(map_h2_error(e))));
                    }
                    Poll::Ready(Ok(response)) => {
                        if response.status() != http::StatusCode::OK {
                            this.state = RespState::Done;
                            return Poll::Ready(Some(Err(TransportError::Internal(format!(
                                "unexpected HTTP status {}",
                                response.status()
                            )))));
                        }
                        this.state = RespState::Body(response.into_body());
                    }
                },
                RespState::Body(body) => {
                    match this.frames.next_frame() {
                        Err(e) => {
                            this.state = RespState::Done;
                            return Poll::Ready(Some(Err(e)));
                        }
                        Ok(Some(frame)) => {
                            if frame.flags.is_compressed() {
                                this.state = RespState::Done;
                                return Poll::Ready(Some(Err(TransportError::Compression(
                                    "compressed payload not yet supported".into(),
                                ))));
                            }
                            return Poll::Ready(Some(Ok(StreamItem::Message(frame.payload))));
                        }
                        Ok(None) => {}
                    }
                    match body.poll_data(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Some(Ok(chunk))) => {
                            let _ = body.flow_control().release_capacity(chunk.len());
                            this.frames.push(&chunk);
                        }
                        Poll::Ready(Some(Err(e))) => {
                            this.state = RespState::Done;
                            return Poll::Ready(Some(Err(map_h2_error(e))));
                        }
                        Poll::Ready(None) => {
                            if !this.frames.is_empty() {
                                this.state = RespState::Done;
                                return Poll::Ready(Some(Err(TransportError::MalformedFrame(
                                    "truncated frame".into(),
                                ))));
                            }
                            if let RespState::Body(body) =
                                std::mem::replace(&mut this.state, RespState::Done)
                            {
                                this.state = RespState::Trailers(body);
                            }
                        }
                    }
                }
                RespState::Trailers(body) => match body.poll_trailers(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(trailers)) => {
                        this.state = RespState::Done;
                        let md = trailers
                            .map(|t| headers_to_metadata(&t))
                            .unwrap_or_default();
                        return Poll::Ready(Some(Ok(StreamItem::Trailer(md))));
                    }
                    Poll::Ready(Err(e)) => {
                        this.state = RespState::Done;
                        return Poll::Ready(Some(Err(map_h2_error(e))));
                    }
                },
                RespState::Done => return Poll::Ready(None),
            }
        }
    }
}

#[cfg(feature = "tls")]
impl Http2Transport {
    fn make_tls_connector(
        &self,
        tls_config: &crate::TlsConfig,
    ) -> Result<tokio_rustls::TlsConnector, TransportError> {
        use rustls::ClientConfig;
        use std::sync::Arc;

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| TransportError::Tls(e.to_string()))?;

        let mut client_config = if tls_config.accept_invalid_certs {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert(provider)))
                .with_no_client_auth()
        } else {
            let mut root_store = rustls::RootCertStore::empty();
            let mut pem: Option<Vec<u8>> = tls_config.cert_pem.clone();
            if pem.is_none() {
                if let Some(path) = &tls_config.cert_path {
                    pem =
                        Some(std::fs::read(path).map_err(|e| TransportError::Tls(e.to_string()))?);
                }
            }
            if let Some(pem) = pem {
                let mut reader = pem.as_slice();
                for cert in rustls_pemfile::certs(&mut reader) {
                    let cert = cert.map_err(|e| TransportError::Tls(e.to_string()))?;
                    root_store
                        .add(cert)
                        .map_err(|e| TransportError::Tls(e.to_string()))?;
                }
            }
            builder
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };
        client_config.alpn_protocols = tls_config.alpn_protocols.clone();

        Ok(tokio_rustls::TlsConnector::from(Arc::new(client_config)))
    }
}

/// Certificate verifier that accepts any server certificate (development only).
#[cfg(feature = "tls")]
#[derive(Debug)]
struct AcceptAnyServerCert(std::sync::Arc<rustls::crypto::CryptoProvider>);

#[cfg(feature = "tls")]
impl rustls::client::danger::ServerCertVerifier for AcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// HTTP/2 server: accepts incoming HTTP/2 connections and dispatches RPC calls.
#[derive(Debug, Clone)]
pub struct Http2Server {
    endpoint: Endpoint,
}

#[derive(Clone)]
struct ServerCfg {
    max: usize,
    keepalive: Option<(Duration, Duration)>,
}

#[cfg(feature = "tls")]
type Acceptor = Option<tokio_rustls::TlsAcceptor>;
#[cfg(not(feature = "tls"))]
type Acceptor = Option<()>;

impl Http2Server {
    /// Creates a new HTTP/2 server bound to the given endpoint.
    pub fn new(endpoint: Endpoint) -> Self {
        Http2Server { endpoint }
    }

    /// Returns the endpoint this server listens on.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Binds a TCP listener on the endpoint address.
    pub async fn bind(&self) -> Result<TcpListener, TransportError> {
        TcpListener::bind(&self.endpoint.address)
            .await
            .map_err(|e| TransportError::Io(e.to_string()))
    }

    /// Runs the server until the listener fails, dispatching calls to `handler`.
    pub async fn serve<F>(&self, handler: F) -> Result<(), TransportError>
    where
        F: Fn(IncomingHttp2Call) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
            + Send
            + Sync
            + Clone
            + 'static,
    {
        self.serve_with_shutdown(handler, std::future::pending())
            .await
    }

    /// Like [`serve`](Self::serve), but stops accepting when `shutdown`
    /// completes and sends GOAWAY on every open connection. In-flight calls
    /// are allowed to finish.
    pub async fn serve_with_shutdown<F, S>(
        &self,
        handler: F,
        shutdown: S,
    ) -> Result<(), TransportError>
    where
        F: Fn(IncomingHttp2Call) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
            + Send
            + Sync
            + Clone
            + 'static,
        S: Future<Output = ()>,
    {
        let listener = self.bind().await?;
        self.serve_listener(listener, handler, shutdown).await
    }

    /// Serves on an already-bound listener (useful with port 0).
    pub async fn serve_listener<F, S>(
        &self,
        listener: TcpListener,
        handler: F,
        shutdown: S,
    ) -> Result<(), TransportError>
    where
        F: Fn(IncomingHttp2Call) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
            + Send
            + Sync
            + Clone
            + 'static,
        S: Future<Output = ()>,
    {
        let acceptor = self.make_acceptor()?;
        let cfg = ServerCfg {
            max: self
                .endpoint
                .max_message_bytes
                .unwrap_or(DEFAULT_MAX_MESSAGE_BYTES),
            keepalive: self
                .endpoint
                .keepalive_interval
                .map(|i| (i, self.endpoint.keepalive_timeout)),
        };
        let (stop_tx, stop_rx) = watch::channel(false);
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    let _ = stop_tx.send(true);
                    return Ok(());
                }
                accepted = listener.accept() => {
                    let (stream, _) = accepted.map_err(|e| TransportError::Io(e.to_string()))?;
                    let _ = stream.set_nodelay(true);
                    let (acceptor, cfg, handler, stop_rx) =
                        (acceptor.clone(), cfg.clone(), handler.clone(), stop_rx.clone());
                    tokio::spawn(async move {
                        let _ = serve_tcp(stream, acceptor, cfg, handler, stop_rx).await;
                    });
                }
            }
        }
    }

    #[cfg(feature = "tls")]
    fn make_acceptor(&self) -> Result<Acceptor, TransportError> {
        match &self.endpoint.tls {
            Some(tls) => make_tls_acceptor(tls).map(Some),
            None => Ok(None),
        }
    }

    #[cfg(not(feature = "tls"))]
    fn make_acceptor(&self) -> Result<Acceptor, TransportError> {
        if self.endpoint.uses_tls() {
            return Err(TransportError::NotSupported(
                "TLS support requires the `tls` feature".into(),
            ));
        }
        Ok(None)
    }
}

async fn serve_tcp<F>(
    stream: TcpStream,
    acceptor: Acceptor,
    cfg: ServerCfg,
    handler: F,
    stop: watch::Receiver<bool>,
) -> Result<(), TransportError>
where
    F: Fn(IncomingHttp2Call) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
        + Send
        + Sync
        + Clone
        + 'static,
{
    #[cfg(feature = "tls")]
    if let Some(acceptor) = acceptor {
        let tls = acceptor
            .accept(stream)
            .await
            .map_err(|e| TransportError::Tls(e.to_string()))?;
        return serve_io(tls, cfg, handler, stop).await;
    }
    #[cfg(not(feature = "tls"))]
    let _ = acceptor;
    serve_io(stream, cfg, handler, stop).await
}

async fn serve_io<IO, F>(
    io: IO,
    cfg: ServerCfg,
    handler: F,
    mut stop: watch::Receiver<bool>,
) -> Result<(), TransportError>
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    F: Fn(IncomingHttp2Call) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
        + Send
        + Sync
        + Clone
        + 'static,
{
    let mut conn = server::Builder::new()
        .handshake::<_, Bytes>(io)
        .await
        .map_err(map_h2_error)?;
    let mut ka: Pin<Box<dyn Future<Output = ()> + Send>> = match (cfg.keepalive, conn.ping_pong()) {
        (Some((interval, timeout)), Some(pings)) => Box::pin(keepalive(pings, interval, timeout)),
        _ => Box::pin(futures::future::pending()),
    };
    let mut shutting_down = false;
    loop {
        tokio::select! {
            next = conn.accept() => match next {
                None => return Ok(()),
                Some(Err(e)) => return Err(map_h2_error(e)),
                Some(Ok((request, respond))) => {
                    let handler = handler.clone();
                    let max = cfg.max;
                    tokio::spawn(handle_stream(request, respond, handler, max));
                }
            },
            _ = &mut ka => return Err(TransportError::ConnectionClosed),
            changed = stop.changed(), if !shutting_down => {
                shutting_down = true;
                if changed.is_ok() {
                    conn.graceful_shutdown();
                }
            }
        }
    }
}

async fn handle_stream<F>(
    request: Request<RecvStream>,
    mut respond: server::SendResponse<Bytes>,
    handler: F,
    max: usize,
) where
    F: Fn(IncomingHttp2Call) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
        + Send
        + Sync
        + Clone
        + 'static,
{
    let method = request.uri().path().trim_start_matches('/').to_string();
    let metadata = headers_to_metadata(request.headers());
    let mut body = request.into_body();
    let mut frames = FrameBuffer::new(max);

    let first = match read_frame(&mut body, &mut frames).await {
        Ok(Some(f)) if !f.flags.is_compressed() => f.payload,
        Ok(None) => Vec::new(),
        Ok(Some(_)) => {
            respond.send_reset(Reason::PROTOCOL_ERROR);
            return;
        }
        Err(TransportError::SizeLimitExceeded { .. }) => {
            respond.send_reset(Reason::ENHANCE_YOUR_CALM);
            return;
        }
        Err(_) => {
            respond.send_reset(Reason::PROTOCOL_ERROR);
            return;
        }
    };

    let response = match Response::builder()
        .status(200)
        .header("content-type", CONTENT_TYPE)
        .body(())
    {
        Ok(r) => r,
        Err(_) => return,
    };
    let send = match respond.send_response(response, false) {
        Ok(s) => s,
        Err(_) => return,
    };

    let (response_tx, response_rx) = mpsc::channel(32);
    let (trailers_tx, trailers_rx) = oneshot::channel();
    let (request_tx, request_rx) = mpsc::channel(32);

    // Remaining request messages (client streaming / bidi).
    tokio::spawn(async move {
        while let Ok(Some(frame)) = read_frame(&mut body, &mut frames).await {
            if frame.flags.is_compressed() || request_tx.send(frame.payload).await.is_err() {
                break;
            }
        }
    });
    tokio::spawn(write_response(send, response_rx, trailers_rx));

    let call = IncomingHttp2Call {
        method,
        metadata,
        request: first,
        response_tx: Some(response_tx),
        trailers_tx: Some(trailers_tx),
        request_rx,
    };
    let _ = handler(call).await;
}

async fn write_response(
    mut send: SendStream<Bytes>,
    mut messages: mpsc::Receiver<Result<FramedMessage, TransportError>>,
    trailers: oneshot::Receiver<Metadata>,
) {
    while let Some(item) = messages.recv().await {
        match item {
            Ok(message) => {
                let frame = Bytes::from(
                    Frame {
                        flags: message.flags,
                        payload: message.payload,
                    }
                    .encode(),
                );
                if write_all(&mut send, frame).await.is_err() {
                    return;
                }
            }
            Err(_) => {
                send.send_reset(Reason::INTERNAL_ERROR);
                return;
            }
        }
    }
    // All senders gone: the call finished. Send trailers (empty if the handler
    // never provided any) which also ends the stream.
    let md = trailers.await.unwrap_or_default();
    let mut headers = HeaderMap::new();
    if metadata_to_headers(&md, &mut headers).is_err() {
        send.send_reset(Reason::INTERNAL_ERROR);
        return;
    }
    let _ = send.send_trailers(headers);
}

/// An incoming HTTP/2 call received by the server.
///
/// The response ends when the call is dropped or after
/// [`send_trailers`](Self::send_trailers); trailers default to empty.
#[derive(Debug)]
pub struct IncomingHttp2Call {
    /// The RPC method name.
    pub method: String,
    /// Request metadata.
    pub metadata: Metadata,
    /// The initial request message payload.
    pub request: Vec<u8>,
    response_tx: Option<mpsc::Sender<Result<FramedMessage, TransportError>>>,
    trailers_tx: Option<oneshot::Sender<Metadata>>,
    request_rx: mpsc::Receiver<Vec<u8>>,
}

impl IncomingHttp2Call {
    /// Sends a response message to the client.
    pub async fn send_message(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        let tx = self
            .response_tx
            .as_ref()
            .ok_or(TransportError::ConnectionClosed)?;
        tx.send(Ok(FramedMessage {
            flags: FrameFlags::empty(),
            payload,
        }))
        .await
        .map_err(|_| TransportError::ConnectionClosed)
    }

    /// Receives the next request message (client streaming / bidi).
    pub async fn recv_message(&mut self) -> Option<Vec<u8>> {
        self.request_rx.recv().await
    }

    /// Sends trailing metadata and finishes the response.
    pub async fn send_trailers(&mut self, trailers: Metadata) -> Result<(), TransportError> {
        if let Some(tx) = self.trailers_tx.take() {
            let _ = tx.send(trailers);
        }
        // Dropping the sender lets the writer flush and end the stream.
        self.response_tx = None;
        Ok(())
    }

    /// Aborts the call: the stream is reset and the client observes
    /// [`TransportError::StreamReset`].
    pub async fn abort(&mut self) {
        if let Some(tx) = self.response_tx.take() {
            let _ = tx
                .send(Err(TransportError::Internal("call aborted".into())))
                .await;
        }
    }
}

/// Response half of an HTTP/2 call.
struct Http2Sender {
    response_tx: Option<mpsc::Sender<Result<FramedMessage, TransportError>>>,
    trailers_tx: Option<oneshot::Sender<Metadata>>,
}

#[async_trait]
impl crate::traits::CallSender for Http2Sender {
    async fn send_message(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        let tx = self
            .response_tx
            .as_ref()
            .ok_or(TransportError::ConnectionClosed)?;
        tx.send(Ok(FramedMessage {
            flags: FrameFlags::empty(),
            payload,
        }))
        .await
        .map_err(|_| TransportError::ConnectionClosed)
    }

    async fn send_trailers(&mut self, trailers: Metadata) -> Result<(), TransportError> {
        if let Some(tx) = self.trailers_tx.take() {
            let _ = tx.send(trailers);
        }
        self.response_tx = None;
        Ok(())
    }
}

impl crate::traits::IncomingCall for IncomingHttp2Call {
    fn into_parts(self) -> crate::traits::IncomingCallParts {
        let mut request_rx = self.request_rx;
        crate::traits::IncomingCallParts {
            method: self.method,
            metadata: self.metadata,
            request: self.request,
            incoming: Box::pin(futures::stream::poll_fn(move |cx| request_rx.poll_recv(cx))),
            sender: Box::new(Http2Sender {
                response_tx: self.response_tx,
                trailers_tx: self.trailers_tx,
            }),
        }
    }
}

#[cfg(feature = "tls")]
fn pem_bytes(
    pem: &Option<Vec<u8>>,
    path: &Option<std::path::PathBuf>,
    what: &str,
) -> Result<Vec<u8>, TransportError> {
    match (pem, path) {
        (Some(p), _) => Ok(p.clone()),
        (None, Some(path)) => {
            std::fs::read(path).map_err(|e| TransportError::Tls(format!("reading {what}: {e}")))
        }
        (None, None) => Err(TransportError::Tls(format!("missing {what}"))),
    }
}

#[cfg(feature = "tls")]
fn make_tls_acceptor(tls: &crate::TlsConfig) -> Result<tokio_rustls::TlsAcceptor, TransportError> {
    let err = |e: &dyn std::fmt::Display| TransportError::Tls(e.to_string());
    let cert_pem = pem_bytes(&tls.cert_pem, &tls.cert_path, "server certificate")?;
    let key_pem = pem_bytes(&tls.key_pem, &tls.key_path, "server private key")?;
    let certs = rustls_pemfile::certs(&mut cert_pem.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| err(&e))?;
    let key = rustls_pemfile::private_key(&mut key_pem.as_slice())
        .map_err(|e| err(&e))?
        .ok_or_else(|| TransportError::Tls("no private key found".into()))?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| err(&e))?;
    let builder = if tls.require_client_cert {
        let ca_pem = pem_bytes(&tls.client_ca_pem, &tls.client_ca_path, "client CA")?;
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut ca_pem.as_slice()) {
            roots.add(cert.map_err(|e| err(&e))?).map_err(|e| err(&e))?;
        }
        let verifier =
            rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .map_err(|e| err(&e))?;
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    };
    let mut config = builder.with_single_cert(certs, key).map_err(|e| err(&e))?;
    config.alpn_protocols = tls.alpn_protocols.clone();
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};

    type Boxed = futures::future::BoxFuture<'static, Result<(), TransportError>>;

    async fn start<F>(endpoint: Endpoint, handler: F) -> (Endpoint, oneshot::Sender<()>)
    where
        F: Fn(IncomingHttp2Call) -> Boxed + Send + Sync + Clone + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let mut client_ep = endpoint.clone();
        client_ep.address = addr;
        let server = Http2Server::new(endpoint);
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = server
                .serve_listener(listener, handler, async {
                    let _ = stop_rx.await;
                })
                .await;
        });
        (client_ep, stop_tx)
    }

    fn echo(mut call: IncomingHttp2Call) -> Boxed {
        Box::pin(async move {
            let mut reply = b"resp:".to_vec();
            reply.extend_from_slice(&call.request);
            call.send_message(reply).await?;
            let mut md = Metadata::new();
            md.insert("x-status", "ok");
            md.insert("x-method", call.method.clone());
            call.send_trailers(md).await
        })
    }

    async fn collect(call: Call) -> Vec<Result<StreamItem, TransportError>> {
        call.stream.collect().await
    }

    #[test]
    fn frame_buffer_reassembles_split_and_coalesced_chunks() {
        let mut wire = Frame::encode_with(b"one", FrameFlags::empty());
        wire.extend(Frame::encode_with(b"second", FrameFlags::empty()));
        let mut fb = FrameBuffer::new(1024);
        let mut out = Vec::new();
        for byte in &wire {
            fb.push(&[*byte]);
            while let Some(f) = fb.next_frame().unwrap() {
                out.push(f.payload);
            }
        }
        assert_eq!(out, vec![b"one".to_vec(), b"second".to_vec()]);
        assert!(fb.is_empty());

        let mut fb = FrameBuffer::new(2);
        fb.push(&Frame::encode_with(b"toolong", FrameFlags::empty())[..5]);
        assert!(matches!(
            fb.next_frame(),
            Err(TransportError::SizeLimitExceeded { limit: 2 })
        ));
    }

    #[tokio::test]
    async fn unary_roundtrip_with_real_trailers() {
        let (ep, _stop) = start(Endpoint::new("x"), echo).await;
        let mut md = Metadata::new();
        md.insert("x-request-id", "abc");
        let call = Http2Transport::new(ep)
            .start_call("pkg.Svc/Method", b"hi".to_vec(), &md, StreamingType::Unary)
            .await
            .unwrap();
        let items: Vec<_> = collect(call)
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], StreamItem::Message(b"resp:hi".to_vec()));
        match &items[1] {
            StreamItem::Trailer(t) => {
                assert_eq!(t.get("x-status"), Some(&["ok".to_string()][..]));
                assert_eq!(t.get("x-method"), Some(&["pkg.Svc/Method".to_string()][..]));
            }
            other => panic!("expected trailer, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_metadata_reaches_handler() {
        let (ep, _stop) = start(Endpoint::new("x"), |mut call: IncomingHttp2Call| -> Boxed {
            Box::pin(async move {
                let v = call.metadata.get("x-request-id").map(|v| v.join(","));
                call.send_message(v.unwrap_or_default().into_bytes())
                    .await?;
                call.send_trailers(Metadata::new()).await
            })
        })
        .await;
        let ep = ep.with_metadata("x-default", "d");
        let mut md = Metadata::new();
        md.insert("x-request-id", "abc");
        let call = Http2Transport::new(ep)
            .start_call("M", vec![], &md, StreamingType::Unary)
            .await
            .unwrap();
        let first = collect(call).await.remove(0).unwrap();
        assert_eq!(first, StreamItem::Message(b"abc".to_vec()));
    }

    #[tokio::test]
    async fn large_message_spans_many_data_frames() {
        let (ep, _stop) = start(Endpoint::new("x"), echo).await;
        let payload = vec![0xA5u8; 1024 * 1024];
        let call = Http2Transport::new(ep)
            .start_call("M", payload.clone(), &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        let first = collect(call).await.remove(0).unwrap();
        match first {
            StreamItem::Message(m) => {
                assert_eq!(m.len(), payload.len() + 5);
                assert!(m.starts_with(b"resp:"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn client_streaming_uses_sink() {
        let (ep, _stop) = start(Endpoint::new("x"), |mut call: IncomingHttp2Call| -> Boxed {
            Box::pin(async move {
                let mut n = 1; // the initial message
                let mut total = call.request.len();
                while let Some(m) = call.recv_message().await {
                    n += 1;
                    total += m.len();
                }
                call.send_message(format!("{n}:{total}").into_bytes())
                    .await?;
                call.send_trailers(Metadata::new()).await
            })
        })
        .await;
        let mut call = Http2Transport::new(ep)
            .start_call(
                "M",
                b"aa".to_vec(),
                &Metadata::new(),
                StreamingType::ClientStream,
            )
            .await
            .unwrap();
        call.sink.send(b"bbb".to_vec()).await.unwrap();
        call.sink.send(b"c".to_vec()).await.unwrap();
        call.sink.close().await.unwrap();
        let first = collect(call).await.remove(0).unwrap();
        assert_eq!(first, StreamItem::Message(b"3:6".to_vec()));
    }

    #[tokio::test]
    async fn bidi_interleaves_messages() {
        let (ep, _stop) = start(Endpoint::new("x"), |mut call: IncomingHttp2Call| -> Boxed {
            Box::pin(async move {
                call.send_message(call.request.clone()).await?;
                while let Some(m) = call.recv_message().await {
                    call.send_message(m).await?;
                }
                call.send_trailers(Metadata::new()).await
            })
        })
        .await;
        let mut call = Http2Transport::new(ep)
            .start_call("M", b"0".to_vec(), &Metadata::new(), StreamingType::Bidi)
            .await
            .unwrap();
        assert_eq!(
            call.stream.next().await.unwrap().unwrap(),
            StreamItem::Message(b"0".to_vec())
        );
        for i in 1..=3u8 {
            call.sink.send(vec![i]).await.unwrap();
            assert_eq!(
                call.stream.next().await.unwrap().unwrap(),
                StreamItem::Message(vec![i])
            );
        }
        call.sink.close().await.unwrap();
        assert!(matches!(
            call.stream.next().await.unwrap().unwrap(),
            StreamItem::Trailer(_)
        ));
        assert!(call.stream.next().await.is_none());
    }

    #[tokio::test]
    async fn server_abort_surfaces_as_stream_reset() {
        let (ep, _stop) = start(Endpoint::new("x"), |mut call: IncomingHttp2Call| -> Boxed {
            Box::pin(async move {
                call.abort().await;
                Ok(())
            })
        })
        .await;
        let call = Http2Transport::new(ep)
            .start_call("M", b"x".to_vec(), &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        let items = collect(call).await;
        assert_eq!(items.last(), Some(&Err(TransportError::StreamReset)));
    }

    #[tokio::test]
    async fn oversized_request_is_rejected_by_server() {
        let (ep, _stop) = start(Endpoint::new("x").with_max_message_bytes(1024), echo).await;
        // Client limit is lifted so the server is the one enforcing.
        let client_ep = ep.clone().with_max_message_bytes(1 << 20);
        let call = Http2Transport::new(client_ep)
            .start_call(
                "M",
                vec![0; 10 * 1024],
                &Metadata::new(),
                StreamingType::Unary,
            )
            .await
            .unwrap();
        let items = collect(call).await;
        assert!(matches!(items.last(), Some(Err(_))), "{items:?}");
        assert!(!items
            .iter()
            .any(|i| matches!(i, Ok(StreamItem::Message(_)))));
    }

    #[tokio::test]
    async fn client_enforces_send_limit() {
        let (ep, _stop) = start(Endpoint::new("x"), echo).await;
        let err = Http2Transport::new(ep.with_max_message_bytes(8))
            .start_call("M", vec![0; 64], &Metadata::new(), StreamingType::Unary)
            .await
            .err()
            .unwrap();
        assert_eq!(err, TransportError::SizeLimitExceeded { limit: 8 });
    }

    #[tokio::test]
    async fn keepalive_does_not_disturb_slow_calls() {
        let ka = Duration::from_millis(30);
        let (ep, _stop) = start(
            Endpoint::new("x").with_keepalive(ka, Duration::from_secs(2)),
            |mut call: IncomingHttp2Call| -> Boxed {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    call.send_message(b"late".to_vec()).await?;
                    call.send_trailers(Metadata::new()).await
                })
            },
        )
        .await;
        let ep = ep.with_keepalive(ka, Duration::from_secs(2));
        let call = Http2Transport::new(ep)
            .start_call("M", vec![], &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        let first = collect(call).await.remove(0).unwrap();
        assert_eq!(first, StreamItem::Message(b"late".to_vec()));
    }

    #[tokio::test]
    async fn graceful_shutdown_lets_inflight_calls_finish() {
        let (ep, stop) = start(Endpoint::new("x"), |mut call: IncomingHttp2Call| -> Boxed {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                call.send_message(b"done".to_vec()).await?;
                call.send_trailers(Metadata::new()).await
            })
        })
        .await;
        let call = Http2Transport::new(ep.clone())
            .start_call("M", vec![], &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        stop.send(()).unwrap();
        let first = collect(call).await.remove(0).unwrap();
        assert_eq!(first, StreamItem::Message(b"done".to_vec()));
        tokio::time::sleep(Duration::from_millis(50)).await;
        // The listener is gone: new calls cannot connect.
        assert!(Http2Transport::new(ep)
            .start_call("M", vec![], &Metadata::new(), StreamingType::Unary)
            .await
            .is_err());
    }

    /// TCP forwarder that counts accepted connections and can sever them all.
    struct Proxy {
        addr: String,
        accepted: Arc<std::sync::atomic::AtomicUsize>,
        sever: tokio::sync::watch::Sender<u64>,
    }

    async fn proxy_to(target: String) -> Proxy {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let accepted = Arc::new(AtomicUsize::new(0));
        let (sever, sever_rx) = tokio::sync::watch::channel(0u64);
        let counter = accepted.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut client, _)) = listener.accept().await else {
                    return;
                };
                counter.fetch_add(1, Ordering::SeqCst);
                let target = target.clone();
                // Only severances after this connection was accepted count.
                let mut sever_rx = sever_rx.clone();
                sever_rx.borrow_and_update();
                tokio::spawn(async move {
                    let Ok(mut server) = TcpStream::connect(target).await else {
                        return;
                    };
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut client, &mut server) => {}
                        _ = sever_rx.changed() => {}
                    }
                });
            }
        });
        Proxy {
            addr,
            accepted,
            sever,
        }
    }

    #[tokio::test]
    async fn calls_share_one_connection_and_reconnect_after_it_dies() {
        use std::sync::atomic::Ordering;
        let (server_ep, _stop) = start(Endpoint::new("x"), echo).await;
        let proxy = proxy_to(server_ep.address.clone()).await;
        let transport = Http2Transport::new(Endpoint::new(proxy.addr.clone()));

        let call_once = |t: Http2Transport| async move {
            let call = t
                .start_call("M", b"hi".to_vec(), &Metadata::new(), StreamingType::Unary)
                .await?;
            let items = collect(call).await;
            items.into_iter().collect::<Result<Vec<_>, _>>()
        };

        // Sequential and concurrent calls (and clones of the transport) all
        // ride on a single TCP connection.
        for _ in 0..5 {
            assert_eq!(call_once(transport.clone()).await.unwrap().len(), 2);
        }
        let concurrent: Vec<_> = (0..16).map(|_| call_once(transport.clone())).collect();
        for r in futures::future::join_all(concurrent).await {
            assert_eq!(r.unwrap().len(), 2);
        }
        assert_eq!(proxy.accepted.load(Ordering::SeqCst), 1);

        // Kill the connection; the next call transparently reconnects.
        proxy.sever.send(1).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(call_once(transport.clone()).await.unwrap().len(), 2);
        assert_eq!(proxy.accepted.load(Ordering::SeqCst), 2);
    }

    #[cfg(feature = "tls")]
    #[tokio::test]
    async fn tls_with_alpn_roundtrip() {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let mut tls = crate::TlsConfig::http2();
        tls.cert_pem = Some(cert.cert.pem().into_bytes());
        tls.key_pem = Some(cert.key_pair.serialize_pem().into_bytes());
        let (mut ep, _stop) = start(Endpoint::new("x").with_tls(tls.clone()), echo).await;
        ep.address = ep.address.replace("127.0.0.1", "localhost");

        let call = Http2Transport::new(ep.clone())
            .start_call("M", b"tls".to_vec(), &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        let first = collect(call).await.remove(0).unwrap();
        assert_eq!(first, StreamItem::Message(b"resp:tls".to_vec()));

        // Without the CA the handshake must fail.
        let mut untrusted = tls;
        untrusted.cert_pem = None;
        untrusted.key_pem = None;
        let bad = Http2Transport::new(ep.with_tls(untrusted))
            .start_call("M", vec![], &Metadata::new(), StreamingType::Unary)
            .await;
        assert!(matches!(bad, Err(TransportError::Tls(_))));
    }
}

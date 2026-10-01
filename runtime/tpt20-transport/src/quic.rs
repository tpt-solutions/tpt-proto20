//! Native QUIC transport (spec §17.3, optional).
//!
//! Every RPC is one bidirectional QUIC stream, so calls multiplex over a single
//! connection without head-of-line blocking between them, connections survive
//! IP changes, and a lost packet stalls only the call it belongs to.
//!
//! This is tpt20's *own* mapping, not gRPC-over-HTTP/3: it does not
//! interoperate with gRPC/HTTP/3 peers (use the HTTP/2 transport with the
//! `GrpcServer` adapter for that). ALPN is [`ALPN`], TLS 1.3 is mandatory.
//!
//! ## Stream format
//!
//! Both directions carry frames `kind:1 | length:4 (big endian) | payload`:
//!
//! | kind | name | payload |
//! | ---- | ---- | ------- |
//! | 1 | `HEADERS` | metadata block; the client's carries `:path` (the method) |
//! | 2 | `MESSAGE` | one message |
//! | 3 | `TRAILERS` | metadata block with the final status (`grpc-status`, …) |
//!
//! The client sends `HEADERS`, then messages, then finishes its send side (a
//! half-close). The server sends messages and exactly one `TRAILERS`, then
//! finishes. Cancelling (dropping the call) stops/resets the stream, which the
//! server observes through [`CallSender::closed_signal`](crate::CallSender).
//! Message compression is not negotiated on QUIC yet.

use crate::endpoint::Endpoint;
use crate::error::TransportError;
use crate::frame::{FrameFlags, FramedMessage};
use crate::http2::{
    client_identity, pem_bytes, AcceptAnyServerCert, IncomingHttp2Call, DEFAULT_MAX_MESSAGE_BYTES,
};
use crate::metadata::Metadata;
use crate::traits::{Call, StreamItem, StreamingType, Transport};
use async_trait::async_trait;
use futures::{Sink, Stream};
use quinn::{RecvStream, SendStream};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch, Mutex};

/// ALPN protocol id of the tpt20 QUIC mapping.
pub const ALPN: &[u8] = b"tpt20/1";

/// A server-side call received over QUIC (same type as the HTTP/2 transport's,
/// so one handler serves both).
pub type IncomingQuicCall = IncomingHttp2Call;

const KIND_HEADERS: u8 = 1;
const KIND_MESSAGE: u8 = 2;
const KIND_TRAILERS: u8 = 3;
const FRAME_HEADER: usize = 5;
const PATH_KEY: &str = ":path";

/// Application error codes used when resetting or closing.
const CODE_CANCELLED: u32 = 1;
const CODE_PROTOCOL: u32 = 2;
const CODE_TOO_BIG: u32 = 3;

// ---- framing ---------------------------------------------------------------

fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(FRAME_HEADER + payload.len());
    out.push(kind);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn read_error(e: quinn::ReadExactError) -> TransportError {
    match e {
        quinn::ReadExactError::FinishedEarly(_) => {
            TransportError::MalformedFrame("stream ended inside a frame".into())
        }
        quinn::ReadExactError::ReadError(quinn::ReadError::Reset(_)) => TransportError::StreamReset,
        quinn::ReadExactError::ReadError(quinn::ReadError::ConnectionLost(_)) => {
            TransportError::ConnectionClosed
        }
        quinn::ReadExactError::ReadError(other) => TransportError::Io(other.to_string()),
    }
}

/// Reads one frame; `Ok(None)` is a clean end of stream at a frame boundary.
async fn read_frame(
    recv: &mut RecvStream,
    max: usize,
) -> Result<Option<(u8, Vec<u8>)>, TransportError> {
    let mut head = [0u8; FRAME_HEADER];
    match recv.read_exact(&mut head).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(e) => return Err(read_error(e)),
    }
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > max {
        return Err(TransportError::SizeLimitExceeded { limit: max });
    }
    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload).await.map_err(read_error)?;
    Ok(Some((head[0], payload)))
}

fn write_error(e: quinn::WriteError) -> TransportError {
    match e {
        quinn::WriteError::Stopped(_) => TransportError::StreamReset,
        quinn::WriteError::ConnectionLost(_) => TransportError::ConnectionClosed,
        other => TransportError::Io(other.to_string()),
    }
}

// ---- metadata blocks -------------------------------------------------------

/// `count:u16 | (klen:u16 key vlen:u32 value)*`; repeated keys are repeated
/// entries.
fn encode_block(path: Option<&str>, md: &Metadata) -> Result<Vec<u8>, TransportError> {
    let mut entries: Vec<(&str, &str)> = Vec::new();
    if let Some(p) = path {
        entries.push((PATH_KEY, p));
    }
    for (k, values) in md.iter() {
        for v in values {
            entries.push((k, v));
        }
    }
    let count = u16::try_from(entries.len())
        .map_err(|_| TransportError::MalformedFrame("too many metadata entries".into()))?;
    let mut out = count.to_be_bytes().to_vec();
    for (k, v) in entries {
        let klen = u16::try_from(k.len())
            .map_err(|_| TransportError::MalformedFrame("metadata key too long".into()))?;
        out.extend_from_slice(&klen.to_be_bytes());
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(&(v.len() as u32).to_be_bytes());
        out.extend_from_slice(v.as_bytes());
    }
    Ok(out)
}

fn decode_block(mut buf: &[u8]) -> Result<(Option<String>, Metadata), TransportError> {
    fn take<'a>(buf: &mut &'a [u8], n: usize) -> Result<&'a [u8], TransportError> {
        if buf.len() < n {
            return Err(TransportError::MalformedFrame("truncated metadata".into()));
        }
        let (head, rest) = buf.split_at(n);
        *buf = rest;
        Ok(head)
    }
    let text = |b: &[u8]| {
        String::from_utf8(b.to_vec())
            .map_err(|_| TransportError::MalformedFrame("metadata is not UTF-8".into()))
    };
    let count = u16::from_be_bytes(take(&mut buf, 2)?.try_into().unwrap_or([0; 2]));
    let mut md = Metadata::new();
    let mut path = None;
    for _ in 0..count {
        let klen = u16::from_be_bytes(take(&mut buf, 2)?.try_into().unwrap_or([0; 2])) as usize;
        let key = text(take(&mut buf, klen)?)?;
        let vlen = u32::from_be_bytes(take(&mut buf, 4)?.try_into().unwrap_or([0; 4])) as usize;
        let value = text(take(&mut buf, vlen)?)?;
        if key == PATH_KEY {
            path = Some(value);
        } else {
            md.insert(key, value);
        }
    }
    if !buf.is_empty() {
        return Err(TransportError::MalformedFrame(
            "trailing bytes after metadata".into(),
        ));
    }
    Ok((path, md))
}

// ---- TLS -------------------------------------------------------------------

fn tls_error(e: impl std::fmt::Display) -> TransportError {
    TransportError::Tls(e.to_string())
}

fn client_crypto(tls: &crate::TlsConfig) -> Result<quinn::ClientConfig, TransportError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls_error)?;
    let identity = client_identity(tls)?;
    let builder = if tls.accept_invalid_certs {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert(provider)))
    } else {
        let mut roots = rustls::RootCertStore::empty();
        let pem = pem_bytes(&tls.cert_pem, &tls.cert_path, "trusted CA certificate")?;
        for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
            roots.add(cert.map_err(tls_error)?).map_err(tls_error)?;
        }
        builder.with_root_certificates(roots)
    };
    let mut config = match identity {
        Some((chain, key)) => builder
            .with_client_auth_cert(chain, key)
            .map_err(tls_error)?,
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![ALPN.to_vec()];
    let quic =
        quinn::crypto::rustls::QuicClientConfig::try_from(Arc::new(config)).map_err(tls_error)?;
    Ok(quinn::ClientConfig::new(Arc::new(quic)))
}

fn server_crypto(endpoint: &Endpoint) -> Result<quinn::ServerConfig, TransportError> {
    let tls = endpoint.tls.as_ref().ok_or_else(|| {
        TransportError::Tls("QUIC always needs TLS: set Endpoint::with_tls".into())
    })?;
    let cert_pem = pem_bytes(&tls.cert_pem, &tls.cert_path, "server certificate")?;
    let key_pem = pem_bytes(&tls.key_pem, &tls.key_path, "server private key")?;
    let certs = rustls_pemfile::certs(&mut cert_pem.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .map_err(tls_error)?;
    let key = rustls_pemfile::private_key(&mut key_pem.as_slice())
        .map_err(tls_error)?
        .ok_or_else(|| TransportError::Tls("no private key found".into()))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls_error)?;
    let builder = if tls.require_client_cert {
        let ca = pem_bytes(&tls.client_ca_pem, &tls.client_ca_path, "client CA")?;
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut ca.as_slice()) {
            roots.add(cert.map_err(tls_error)?).map_err(tls_error)?;
        }
        let verifier =
            rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .map_err(tls_error)?;
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    };
    let mut config = builder.with_single_cert(certs, key).map_err(tls_error)?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    let quic =
        quinn::crypto::rustls::QuicServerConfig::try_from(Arc::new(config)).map_err(tls_error)?;
    let mut server = quinn::ServerConfig::with_crypto(Arc::new(quic));
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(endpoint.max_concurrent_streams.into());
    transport.max_concurrent_uni_streams(0u32.into());
    if let Some(interval) = endpoint.keepalive_interval {
        transport.keep_alive_interval(Some(interval));
        if let Ok(idle) = (interval + endpoint.keepalive_timeout).try_into() {
            transport.max_idle_timeout(Some(idle));
        }
    }
    server.transport_config(Arc::new(transport));
    Ok(server)
}

// ---- client ----------------------------------------------------------------

/// Client-side QUIC transport: one pooled connection, one stream per call.
#[derive(Clone)]
pub struct QuicTransport {
    endpoint: Endpoint,
    state: Arc<Mutex<ClientState>>,
}

#[derive(Default)]
struct ClientState {
    socket: Option<quinn::Endpoint>,
    connection: Option<quinn::Connection>,
}

impl std::fmt::Debug for QuicTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuicTransport")
            .field("endpoint", &self.endpoint.address)
            .finish_non_exhaustive()
    }
}

impl QuicTransport {
    /// Creates a transport for `endpoint` (which needs a [`TlsConfig`](crate::TlsConfig)
    /// whose certificate is the CA to trust).
    pub fn new(endpoint: Endpoint) -> Self {
        QuicTransport {
            endpoint,
            state: Arc::default(),
        }
    }

    /// The endpoint this transport connects to.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    async fn connection(&self) -> Result<quinn::Connection, TransportError> {
        let mut st = self.state.lock().await;
        if let Some(c) = &st.connection {
            if c.close_reason().is_none() {
                return Ok(c.clone());
            }
        }
        let tls = self
            .endpoint
            .tls
            .as_ref()
            .ok_or_else(|| TransportError::Tls("QUIC always needs TLS".into()))?;
        let address = self.endpoint.address.clone();
        let addr: SocketAddr = tokio::net::lookup_host(&address)
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?
            .next()
            .ok_or_else(|| TransportError::Io(format!("cannot resolve {address}")))?;
        let host = address
            .rsplit_once(':')
            .map_or(address.as_str(), |(h, _)| h);
        if st.socket.is_none() {
            let bind: SocketAddr = if addr.is_ipv6() {
                ([0u16; 8], 0).into()
            } else {
                ([0u8; 4], 0).into()
            };
            st.socket =
                Some(quinn::Endpoint::client(bind).map_err(|e| TransportError::Io(e.to_string()))?);
        }
        let mut config = client_crypto(tls)?;
        let mut transport = quinn::TransportConfig::default();
        if let Some(interval) = self.endpoint.keepalive_interval {
            transport.keep_alive_interval(Some(interval));
        }
        config.transport_config(Arc::new(transport));
        let connecting = st
            .socket
            .as_ref()
            .ok_or(TransportError::ConnectionClosed)?
            .connect_with(config, addr, host)
            .map_err(|e| TransportError::Io(e.to_string()))?;
        let conn = connecting
            .await
            .map_err(|e| TransportError::Tls(e.to_string()))?;
        st.connection = Some(conn.clone());
        Ok(conn)
    }

    async fn discard(&self) {
        self.state.lock().await.connection = None;
    }
}

#[async_trait]
impl Transport for QuicTransport {
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
        if request.len() > max {
            return Err(TransportError::SizeLimitExceeded { limit: max });
        }
        let mut merged = self.endpoint.default_metadata.clone();
        merged.merge(metadata.clone());
        let headers = frame(KIND_HEADERS, &encode_block(Some(method), &merged)?);
        let first = (!request.is_empty()
            || matches!(
                streaming_type,
                StreamingType::Unary | StreamingType::ServerStream
            ))
        .then(|| frame(KIND_MESSAGE, &request));

        // One transparent retry on a fresh connection if the cached one died
        // before anything was sent.
        let mut attempt = 0;
        let (mut send, recv) = loop {
            let conn = self.connection().await?;
            match conn.open_bi().await {
                Ok(pair) => break pair,
                Err(_) if attempt == 0 => {
                    attempt += 1;
                    self.discard().await;
                }
                Err(_) => return Err(TransportError::ConnectionClosed),
            }
        };
        send.write_all(&headers).await.map_err(write_error)?;
        if let Some(first) = &first {
            send.write_all(first).await.map_err(write_error)?;
        }
        let single = matches!(
            streaming_type,
            StreamingType::Unary | StreamingType::ServerStream
        );
        if single {
            send.finish().map_err(|_| TransportError::StreamReset)?;
        }

        // A reader task turns frames into stream items; dropping the response
        // stream aborts it, which drops the receive half (STOP_SENDING) so the
        // server learns the call was cancelled.
        let (tx, rx) = mpsc::channel(32);
        let reader = tokio::spawn(async move {
            let mut recv = recv;
            loop {
                match read_frame(&mut recv, max).await {
                    Ok(Some((KIND_MESSAGE, payload))) => {
                        if tx.send(Ok(StreamItem::Message(payload))).await.is_err() {
                            return;
                        }
                    }
                    Ok(Some((KIND_TRAILERS, block))) => {
                        let item = decode_block(&block).map(|(_, md)| StreamItem::Trailer(md));
                        let _ = tx.send(item).await;
                        return;
                    }
                    Ok(Some((KIND_HEADERS, _))) => {} // initial metadata: not used yet
                    Ok(Some(_)) => {
                        let _ = tx
                            .send(Err(TransportError::MalformedFrame(
                                "unknown frame kind".into(),
                            )))
                            .await;
                        return;
                    }
                    Ok(None) => return,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                }
            }
        });
        Ok(Call {
            sink: Box::pin(QuicSink {
                send,
                pending: Vec::new(),
                max,
                ended: single,
            }),
            stream: Box::pin(QuicResponseStream {
                rx,
                reader: Some(reader),
            }),
        })
    }
}

struct QuicResponseStream {
    rx: mpsc::Receiver<Result<StreamItem, TransportError>>,
    reader: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for QuicResponseStream {
    fn drop(&mut self) {
        if let Some(r) = self.reader.take() {
            r.abort();
        }
    }
}

impl Stream for QuicResponseStream {
    type Item = Result<StreamItem, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Request sink: frames are buffered and pushed into the QUIC stream.
struct QuicSink {
    send: SendStream,
    pending: Vec<u8>,
    max: usize,
    ended: bool,
}

impl QuicSink {
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), TransportError>> {
        while !self.pending.is_empty() {
            match Pin::new(&mut self.send).poll_write(cx, &self.pending) {
                Poll::Ready(Ok(n)) => {
                    self.pending.drain(..n);
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Err(TransportError::Io(e.to_string())));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl Sink<Vec<u8>> for QuicSink {
    type Error = TransportError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_drain(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        if self.ended {
            return Err(TransportError::InvalidState(
                "request stream already finished".into(),
            ));
        }
        if item.len() > self.max {
            return Err(TransportError::SizeLimitExceeded { limit: self.max });
        }
        let framed = frame(KIND_MESSAGE, &item);
        self.pending.extend_from_slice(&framed);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_drain(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.poll_drain(cx) {
            Poll::Ready(Ok(())) => {}
            other => return other,
        }
        if !self.ended {
            self.ended = true;
            // Half-close; an already finished stream is fine.
            let _ = self.send.finish();
        }
        Poll::Ready(Ok(()))
    }
}

// ---- server ----------------------------------------------------------------

/// Server-side QUIC transport.
#[derive(Debug, Clone)]
pub struct QuicServer {
    endpoint: Endpoint,
}

type Handler = Arc<
    dyn Fn(IncomingQuicCall) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
        + Send
        + Sync,
>;

#[derive(Clone)]
struct StreamCfg {
    max: usize,
    max_header: usize,
}

impl QuicServer {
    /// Creates a server for `endpoint`; its [`TlsConfig`](crate::TlsConfig)
    /// must carry the certificate and key (QUIC is always encrypted).
    pub fn new(endpoint: Endpoint) -> Self {
        QuicServer { endpoint }
    }

    /// The endpoint this server was configured with.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Binds the UDP socket named by `endpoint.address`.
    pub fn bind(&self) -> Result<quinn::Endpoint, TransportError> {
        let addr: SocketAddr = self
            .endpoint
            .address
            .parse()
            .map_err(|e| TransportError::Io(format!("bad listen address: {e}")))?;
        quinn::Endpoint::server(server_crypto(&self.endpoint)?, addr)
            .map_err(|e| TransportError::Io(e.to_string()))
    }

    /// Binds and serves until the socket fails.
    pub async fn serve<F>(&self, handler: F) -> Result<(), TransportError>
    where
        F: Fn(IncomingQuicCall) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
            + Send
            + Sync
            + 'static,
    {
        let socket = self.bind()?;
        self.serve_socket(socket, handler, std::future::pending())
            .await
    }

    /// Serves on an existing socket (useful with port 0) until `shutdown`
    /// completes. In-flight calls finish; no new ones are accepted.
    pub async fn serve_socket<F, S>(
        &self,
        socket: quinn::Endpoint,
        handler: F,
        shutdown: S,
    ) -> Result<(), TransportError>
    where
        F: Fn(IncomingQuicCall) -> futures::future::BoxFuture<'static, Result<(), TransportError>>
            + Send
            + Sync
            + 'static,
        S: Future<Output = ()>,
    {
        let handler: Handler = Arc::new(handler);
        let cfg = StreamCfg {
            max: self
                .endpoint
                .max_message_bytes
                .unwrap_or(DEFAULT_MAX_MESSAGE_BYTES),
            max_header: self.endpoint.max_header_list_bytes as usize,
        };
        let slots = self
            .endpoint
            .max_connections
            .map(|n| Arc::new(tokio::sync::Semaphore::new(n)));
        let handshake_timeout = self.endpoint.handshake_timeout;
        let (stop_tx, stop_rx) = watch::channel(false);
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    let _ = stop_tx.send(true);
                    return Ok(());
                }
                incoming = socket.accept() => {
                    let Some(incoming) = incoming else { return Ok(()) };
                    let permit = match &slots {
                        Some(s) => match s.clone().try_acquire_owned() {
                            Ok(p) => Some(p),
                            Err(_) => { incoming.refuse(); continue; }
                        },
                        None => None,
                    };
                    let (handler, cfg, stop) = (handler.clone(), cfg.clone(), stop_rx.clone());
                    tokio::spawn(async move {
                        let accepted = tokio::time::timeout(handshake_timeout, async {
                            incoming.accept().map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())
                        }).await;
                        if let Ok(Ok(conn)) = accepted {
                            serve_connection(conn, handler, cfg, stop).await;
                        }
                        drop(permit);
                    });
                }
            }
        }
    }
}

async fn serve_connection(
    conn: quinn::Connection,
    handler: Handler,
    cfg: StreamCfg,
    mut stop: watch::Receiver<bool>,
) {
    let mut streams = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = conn.accept_bi() => match accepted {
                Ok((send, recv)) => {
                    streams.spawn(handle_stream(send, recv, handler.clone(), cfg.clone()));
                }
                Err(_) => break,
            },
            // Reap finished streams so the set does not grow without bound.
            Some(_) = streams.join_next(), if !streams.is_empty() => {}
            changed = stop.changed() => {
                let _ = changed;
                break;
            }
        }
    }
    // Let running calls finish (graceful shutdown) unless the peer is gone.
    while streams.join_next().await.is_some() {}
    conn.close(0u32.into(), b"bye");
}

async fn handle_stream(
    mut send: SendStream,
    mut recv: RecvStream,
    handler: Handler,
    cfg: StreamCfg,
) {
    // HEADERS first, bounded by the header-size limit.
    let (path, metadata) = match read_frame(&mut recv, cfg.max_header).await {
        Ok(Some((KIND_HEADERS, block))) => match decode_block(&block) {
            Ok((Some(path), md)) => (path, md),
            _ => {
                let _ = send.reset(CODE_PROTOCOL.into());
                return;
            }
        },
        Err(TransportError::SizeLimitExceeded { .. }) => {
            let _ = send.reset(CODE_TOO_BIG.into());
            return;
        }
        _ => {
            let _ = send.reset(CODE_PROTOCOL.into());
            return;
        }
    };
    let (first, request_present) = match read_frame(&mut recv, cfg.max).await {
        Ok(Some((KIND_MESSAGE, payload))) => (payload, true),
        Ok(None) => (Vec::new(), false),
        Err(TransportError::SizeLimitExceeded { .. }) => {
            let _ = send.reset(CODE_TOO_BIG.into());
            return;
        }
        _ => {
            let _ = send.reset(CODE_PROTOCOL.into());
            return;
        }
    };

    let (response_tx, response_rx) = mpsc::channel(32);
    let (trailers_tx, trailers_rx) = oneshot::channel();
    let (request_tx, request_rx) = mpsc::channel(32);
    let (closed_tx, closed_rx) = watch::channel(false);

    let max = cfg.max;
    tokio::spawn(async move {
        while let Ok(Some((kind, payload))) = read_frame(&mut recv, max).await {
            if kind != KIND_MESSAGE || request_tx.send(payload).await.is_err() {
                break;
            }
        }
    });
    let writer = tokio::spawn(write_response(send, response_rx, trailers_rx, closed_tx));

    let call = IncomingQuicCall::from_channels(
        path,
        metadata,
        first,
        request_present,
        response_tx,
        trailers_tx,
        request_rx,
        closed_rx,
    );
    let _ = handler(call).await;
    // The call is only over once its response has been flushed; graceful
    // shutdown relies on this before it closes the connection.
    let _ = writer.await;
}

async fn write_response(
    mut send: SendStream,
    mut messages: mpsc::Receiver<Result<FramedMessage, TransportError>>,
    trailers: oneshot::Receiver<Metadata>,
    closed: watch::Sender<bool>,
) {
    loop {
        // Notice the client cancelling even while the handler is idle.
        let item = tokio::select! {
            item = messages.recv() => item,
            _ = send.stopped() => {
                let _ = closed.send(true);
                return;
            }
        };
        match item {
            Some(Ok(msg)) => {
                debug_assert_eq!(msg.flags, FrameFlags::empty());
                if send
                    .write_all(&frame(KIND_MESSAGE, &msg.payload))
                    .await
                    .is_err()
                {
                    let _ = closed.send(true);
                    return;
                }
            }
            Some(Err(_)) => {
                // Handler aborted the call.
                let _ = send.reset(CODE_CANCELLED.into());
                return;
            }
            None => break,
        }
    }
    let block = match trailers.await {
        Ok(md) => encode_block(None, &md).unwrap_or_default(),
        Err(_) => {
            let _ = send.reset(CODE_CANCELLED.into());
            return;
        }
    };
    if send.write_all(&frame(KIND_TRAILERS, &block)).await.is_ok() {
        let _ = send.finish();
        // Give the peer time to read everything before the stream is dropped.
        let _ = tokio::time::timeout(Duration::from_secs(10), send.stopped()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};

    type Boxed = futures::future::BoxFuture<'static, Result<(), TransportError>>;

    fn tls() -> crate::TlsConfig {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let mut tls = crate::TlsConfig::http2();
        tls.cert_pem = Some(cert.cert.pem().into_bytes());
        tls.key_pem = Some(cert.key_pair.serialize_pem().into_bytes());
        tls
    }

    /// Starts a server on an ephemeral UDP port; returns the client endpoint.
    async fn start<F>(endpoint: Endpoint, handler: F) -> (Endpoint, oneshot::Sender<()>)
    where
        F: Fn(IncomingQuicCall) -> Boxed + Send + Sync + 'static,
    {
        let mut server_ep = endpoint.clone();
        server_ep.address = "127.0.0.1:0".into();
        let server = QuicServer::new(server_ep);
        let socket = server.bind().unwrap();
        let port = socket.local_addr().unwrap().port();
        let mut client_ep = endpoint;
        client_ep.address = format!("localhost:{port}");
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = server
                .serve_socket(socket, handler, async {
                    let _ = stop_rx.await;
                })
                .await;
        });
        (client_ep, stop_tx)
    }

    fn echo(mut call: IncomingQuicCall) -> Boxed {
        Box::pin(async move {
            let mut reply = b"resp:".to_vec();
            reply.extend_from_slice(&call.request);
            call.send_message(reply).await?;
            let mut md = Metadata::new();
            md.insert("x-status", "ok");
            md.insert("x-method", call.method.clone());
            if let Some(v) = call.metadata.get("x-in") {
                md.insert("x-echo", v.join(","));
            }
            call.send_trailers(md).await
        })
    }

    fn client(ep: &Endpoint) -> QuicTransport {
        // The client trusts the server's self-signed certificate.
        QuicTransport::new(ep.clone())
    }

    async fn collect(call: Call) -> Vec<Result<StreamItem, TransportError>> {
        call.stream.collect().await
    }

    #[test]
    fn metadata_blocks_roundtrip_and_reject_garbage() {
        let mut md = Metadata::new();
        md.insert("a", "1");
        md.insert("a", "2");
        md.insert("b-bin", "xyz");
        let block = encode_block(Some("pkg.S/M"), &md).unwrap();
        let (path, back) = decode_block(&block).unwrap();
        assert_eq!(path.as_deref(), Some("pkg.S/M"));
        assert_eq!(back.get("a").unwrap(), ["1", "2"]);
        assert_eq!(back.get("b-bin").unwrap(), ["xyz"]);
        for n in 0..block.len() {
            assert!(decode_block(&block[..n]).is_err(), "prefix {n}");
        }
        let mut extra = block.clone();
        extra.push(0);
        assert!(decode_block(&extra).is_err());
        assert!(decode_block(&[0xff, 0xff]).is_err());
    }

    #[tokio::test]
    async fn unary_roundtrip_with_trailers_and_metadata() {
        let (ep, _stop) = start(Endpoint::new("x").with_tls(tls()), echo).await;
        let mut md = Metadata::new();
        md.insert("x-in", "hello");
        let call = client(&ep)
            .start_call("pkg.Svc/Echo", b"ping".to_vec(), &md, StreamingType::Unary)
            .await
            .unwrap();
        let items = collect(call).await;
        assert_eq!(items[0], Ok(StreamItem::Message(b"resp:ping".to_vec())));
        match &items[1] {
            Ok(StreamItem::Trailer(t)) => {
                assert_eq!(t.get("x-status").unwrap(), ["ok"]);
                assert_eq!(t.get("x-method").unwrap(), ["pkg.Svc/Echo"]);
                assert_eq!(t.get("x-echo").unwrap(), ["hello"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn untrusted_server_certificate_is_rejected() {
        let (mut ep, _stop) = start(Endpoint::new("x").with_tls(tls()), echo).await;
        ep.tls = Some(tls()); // a different, untrusted certificate
        let res = client(&ep)
            .start_call("M", vec![], &Metadata::new(), StreamingType::Unary)
            .await;
        assert!(
            matches!(res, Err(TransportError::Tls(_))),
            "{:?}",
            res.map(|_| ())
        );
    }

    #[tokio::test]
    async fn bidi_streaming_interleaves_messages() {
        let handler = |mut call: IncomingQuicCall| -> Boxed {
            Box::pin(async move {
                let first = std::mem::take(&mut call.request);
                call.send_message([b"e:".as_slice(), &first].concat())
                    .await?;
                while let Some(m) = call.recv_message().await {
                    call.send_message([b"e:".as_slice(), &m].concat()).await?;
                }
                call.send_trailers(Metadata::new()).await
            })
        };
        let (ep, _stop) = start(Endpoint::new("x").with_tls(tls()), handler).await;
        let mut call = client(&ep)
            .start_call("M", b"0".to_vec(), &Metadata::new(), StreamingType::Bidi)
            .await
            .unwrap();
        assert_eq!(
            call.stream.next().await,
            Some(Ok(StreamItem::Message(b"e:0".to_vec())))
        );
        for i in 1..=3u8 {
            call.sink.send(vec![b'0' + i]).await.unwrap();
            assert_eq!(
                call.stream.next().await,
                Some(Ok(StreamItem::Message(vec![b'e', b':', b'0' + i])))
            );
        }
        call.sink.close().await.unwrap();
        assert!(matches!(
            call.stream.next().await,
            Some(Ok(StreamItem::Trailer(_)))
        ));
        assert!(call.stream.next().await.is_none());
    }

    #[tokio::test]
    async fn many_concurrent_calls_share_one_connection() {
        let (ep, _stop) = start(Endpoint::new("x").with_tls(tls()), echo).await;
        let t = Arc::new(client(&ep));
        let mut tasks = Vec::new();
        for i in 0..64u8 {
            let t = t.clone();
            tasks.push(tokio::spawn(async move {
                let call = t
                    .start_call("M", vec![i], &Metadata::new(), StreamingType::Unary)
                    .await
                    .unwrap();
                let items = collect(call).await;
                assert_eq!(
                    items[0],
                    Ok(StreamItem::Message([b"resp:".as_slice(), &[i]].concat()))
                );
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
    }

    #[tokio::test]
    async fn large_messages_cross_many_packets() {
        let (ep, _stop) = start(Endpoint::new("x").with_tls(tls()), echo).await;
        let big = vec![0xab; 3 * 1024 * 1024];
        let call = client(&ep)
            .start_call("M", big.clone(), &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        let items = collect(call).await;
        match &items[0] {
            Ok(StreamItem::Message(m)) => {
                assert_eq!(m.len(), big.len() + 5);
                assert!(m[5..].iter().all(|b| *b == 0xab));
            }
            other => panic!("{:?}", other.as_ref().map(|_| ())),
        }
    }

    #[tokio::test]
    async fn size_limits_apply_on_both_sides() {
        let (ep, _stop) = start(
            Endpoint::new("x")
                .with_tls(tls())
                .with_max_message_bytes(1024),
            echo,
        )
        .await;
        // The client refuses to send oversized requests…
        let small_client = QuicTransport::new(ep.clone());
        let e = small_client
            .start_call("M", vec![0; 4096], &Metadata::new(), StreamingType::Unary)
            .await;
        assert!(matches!(e, Err(TransportError::SizeLimitExceeded { .. })));
        // …and the server resets oversized ones from a client with a bigger limit.
        let big_client = QuicTransport::new(ep.clone().with_max_message_bytes(1 << 20));
        let call = big_client
            .start_call("M", vec![0; 4096], &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        let items = collect(call).await;
        assert!(items.last().is_some_and(|i| i.is_err()), "{items:?}");
    }

    #[tokio::test]
    async fn dropping_the_call_is_seen_by_the_server() {
        let (seen_tx, seen_rx) = oneshot::channel::<()>();
        let seen_tx = Arc::new(std::sync::Mutex::new(Some(seen_tx)));
        let handler = move |call: IncomingQuicCall| -> Boxed {
            let seen_tx = seen_tx.clone();
            Box::pin(async move {
                use crate::traits::IncomingCall;
                let parts = call.into_parts();
                parts.sender.closed_signal().await;
                if let Some(tx) = seen_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                Ok(())
            })
        };
        let (ep, _stop) = start(Endpoint::new("x").with_tls(tls()), handler).await;
        let call = client(&ep)
            .start_call("M", b"x".to_vec(), &Metadata::new(), StreamingType::Bidi)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(call);
        tokio::time::timeout(Duration::from_secs(5), seen_rx)
            .await
            .expect("server noticed the cancellation")
            .unwrap();
    }

    #[tokio::test]
    async fn server_abort_surfaces_as_stream_reset() {
        let handler = |mut call: IncomingQuicCall| -> Boxed {
            Box::pin(async move {
                call.abort().await;
                Ok(())
            })
        };
        let (ep, _stop) = start(Endpoint::new("x").with_tls(tls()), handler).await;
        let call = client(&ep)
            .start_call("M", b"x".to_vec(), &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        let items = collect(call).await;
        assert_eq!(items.last(), Some(&Err(TransportError::StreamReset)));
    }

    #[tokio::test]
    async fn oversized_metadata_is_rejected_before_the_handler() {
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        let ep = Endpoint::new("x")
            .with_tls(tls())
            .with_max_header_list_bytes(1024);
        let (ep, _stop) = start(ep, move |call| {
            h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            echo(call)
        })
        .await;
        let mut md = Metadata::new();
        md.insert("x-big", "a".repeat(64 * 1024));
        let call = client(&ep)
            .start_call("M", b"x".to_vec(), &md, StreamingType::Unary)
            .await
            .unwrap();
        let items = collect(call).await;
        assert!(items.last().is_some_and(|i| i.is_err()), "{items:?}");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn reconnects_after_the_connection_closes() {
        let (ep, _stop) = start(Endpoint::new("x").with_tls(tls()), echo).await;
        let t = client(&ep);
        let ok = |t: &QuicTransport| {
            let t = t.clone();
            async move {
                let call = t
                    .start_call("M", b"a".to_vec(), &Metadata::new(), StreamingType::Unary)
                    .await
                    .unwrap();
                matches!(collect(call).await[0], Ok(StreamItem::Message(_)))
            }
        };
        assert!(ok(&t).await);
        t.state
            .lock()
            .await
            .connection
            .as_ref()
            .unwrap()
            .close(9u32.into(), b"test");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(ok(&t).await);
    }

    #[tokio::test]
    async fn mtls_requires_a_trusted_client_certificate() {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issue = |names: Vec<String>| {
            let key = rcgen::KeyPair::generate().unwrap();
            let cert = rcgen::CertificateParams::new(names)
                .unwrap()
                .signed_by(&key, &ca, &ca_key)
                .unwrap();
            (cert.pem().into_bytes(), key.serialize_pem().into_bytes())
        };
        let (server_cert, server_key) = issue(vec!["localhost".into()]);
        let (client_cert, client_key) = issue(vec!["client".into()]);
        let ca_pem = ca.pem().into_bytes();

        let mut server_tls = crate::TlsConfig::http2().with_client_ca_pem(ca_pem.clone());
        server_tls.cert_pem = Some(server_cert);
        server_tls.key_pem = Some(server_key);
        let (ep, _stop) = start(Endpoint::new("x").with_tls(server_tls), echo).await;

        let mut trust = crate::TlsConfig::http2();
        trust.cert_pem = Some(ca_pem);

        let anonymous = client(&ep.clone().with_tls(trust.clone()))
            .start_call("M", b"x".to_vec(), &Metadata::new(), StreamingType::Unary)
            .await;
        let denied = match anonymous {
            Err(_) => true,
            Ok(call) => collect(call).await.iter().any(|i| i.is_err()),
        };
        assert!(denied, "anonymous client got through");

        let identified = trust.with_client_identity_pem(client_cert, client_key);
        let call = client(&ep.with_tls(identified))
            .start_call("M", b"ok".to_vec(), &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        assert_eq!(
            collect(call).await[0],
            Ok(StreamItem::Message(b"resp:ok".to_vec()))
        );
    }

    #[tokio::test]
    async fn graceful_shutdown_lets_inflight_calls_finish() {
        let handler = |mut call: IncomingQuicCall| -> Boxed {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                call.send_message(b"late".to_vec()).await?;
                call.send_trailers(Metadata::new()).await
            })
        };
        let (ep, stop) = start(Endpoint::new("x").with_tls(tls()), handler).await;
        let call = client(&ep)
            .start_call("M", b"x".to_vec(), &Metadata::new(), StreamingType::Unary)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = stop.send(());
        let items = collect(call).await;
        assert_eq!(items[0], Ok(StreamItem::Message(b"late".to_vec())));
    }
}

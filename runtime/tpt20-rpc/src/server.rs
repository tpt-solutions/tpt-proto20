//! RPC server runtime (spec §16): a [`Server`] routes incoming transport
//! calls to [`Service`]s; generated service wrappers implement [`Service`]
//! and use [`ServerCall`]'s typed drivers.
//!
//! The server builds each call's [`RpcContext`] from the wire (metadata,
//! `grpc-timeout` deadline), enforces the client's deadline, cancels the
//! context when the client goes away, and always terminates the call with a
//! status trailer — even if a handler fails to, or panics.

use crate::cancellation::CancellationToken;
use crate::context::RpcContext;
use crate::deadline::Deadline;
use crate::error::RpcError;
use crate::observe::{CallInfo, CallObserver, Side};
use crate::wire;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt};
use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tpt20_core::DecodeError;
use tpt20_transport::{CallSender, IncomingCall, RequestStream};

/// A routable service (implemented by generated `XServer` wrappers).
#[async_trait]
pub trait Service: Send + Sync + 'static {
    /// Fully qualified service name, e.g. `user.v1.UserService`.
    fn name(&self) -> &'static str;

    /// Handles one call to `method` (the part of the path after `/`).
    /// Implementations must end the call with [`ServerCall::finish`] (the
    /// typed drivers do this).
    async fn handle(&self, method: &str, call: ServerCall);
}

struct SenderInner {
    sender: Mutex<Box<dyn CallSender>>,
    finished: AtomicBool,
    cancel: CancellationToken,
    obs: CallObserver,
}

/// Writes responses for one call. Cheap to clone; the final status is sent
/// at most once.
#[derive(Clone)]
pub struct ResponseSender {
    inner: Arc<SenderInner>,
}

impl ResponseSender {
    fn new(sender: Box<dyn CallSender>, cancel: CancellationToken, obs: CallObserver) -> Self {
        ResponseSender {
            inner: Arc::new(SenderInner {
                sender: Mutex::new(sender),
                finished: AtomicBool::new(false),
                cancel,
                obs,
            }),
        }
    }

    fn observer(&self) -> &CallObserver {
        &self.inner.obs
    }

    /// Sends one response message. A failure means the client is gone and
    /// also cancels the call's context.
    pub async fn send(&self, payload: Vec<u8>) -> Result<(), RpcError> {
        if self.inner.finished.load(Ordering::SeqCst) {
            return Err(RpcError::failed_precondition("call already finished").finish());
        }
        let len = payload.len();
        let sender = self.inner.sender.lock().await;
        sender.send_message(payload).await.map_err(|e| {
            self.inner.cancel.cancel();
            self.inner.obs.transport_error(&e);
            wire::transport_error(e)
        })?;
        self.inner.obs.message_sent(len);
        Ok(())
    }

    /// Ends the call with `result` as its final status (first call wins).
    pub async fn finish(&self, result: Result<(), RpcError>) {
        if self.inner.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        self.inner.obs.finish_result(&result);
        let mut sender = self.inner.sender.lock().await;
        let _ = sender.send_trailers(wire::status_trailers(&result)).await;
    }
}

/// One incoming call as seen by a [`Service`].
pub struct ServerCall {
    ctx: RpcContext,
    request: Vec<u8>,
    /// Whether the client sent a first message (always for unary and
    /// server-streaming calls; optional for client-streaming and bidi ones).
    request_present: bool,
    incoming: RequestStream,
    sender: ResponseSender,
}

impl ServerCall {
    /// All request messages of a streaming call, including the one that
    /// opened it (if any).
    fn request_messages(&mut self) -> RequestStream {
        let rest = std::mem::replace(&mut self.incoming, futures::stream::empty().boxed());
        if self.request_present {
            let first = std::mem::take(&mut self.request);
            futures::stream::once(async move { first })
                .chain(rest)
                .boxed()
        } else {
            rest
        }
    }
}

impl ServerCall {
    /// The call context (metadata, deadline, cancellation).
    pub fn ctx(&self) -> &RpcContext {
        &self.ctx
    }

    /// A handle for sending responses / the final status from other tasks.
    pub fn sender(&self) -> ResponseSender {
        self.sender.clone()
    }

    /// Ends the call with `result`.
    pub async fn finish(self, result: Result<(), RpcError>) {
        self.sender.finish(result).await;
    }

    /// Runs a unary handler.
    pub async fn unary<Req, Resp, D, E, F, Fut>(self, decode: D, encode: E, handler: F)
    where
        Req: Send,
        Resp: Send,
        D: Fn(&[u8]) -> Result<Req, DecodeError> + Send + Sync,
        E: Fn(&Resp) -> Vec<u8> + Send + Sync,
        F: FnOnce(RpcContext, Req) -> Fut + Send,
        Fut: Future<Output = Result<Resp, RpcError>> + Send,
    {
        self.sender.observer().set_streaming("unary");
        let result = async {
            let req = decode_request(&decode, &self.request, self.sender.observer())?;
            let resp = handler(self.ctx.clone(), req).await?;
            self.sender.send(encode(&resp)).await
        }
        .await;
        self.finish(result).await;
    }

    /// Runs a server-streaming handler.
    pub async fn server_streaming<Req, Resp, D, E, F, Fut>(self, decode: D, encode: E, handler: F)
    where
        Req: Send,
        Resp: Send + 'static,
        D: Fn(&[u8]) -> Result<Req, DecodeError> + Send + Sync,
        E: Fn(&Resp) -> Vec<u8> + Send + Sync,
        F: FnOnce(RpcContext, Req) -> Fut + Send,
        Fut: Future<Output = Result<BoxStream<'static, Result<Resp, RpcError>>, RpcError>> + Send,
    {
        self.sender.observer().set_streaming("server_streaming");
        let result = async {
            let req = decode_request(&decode, &self.request, self.sender.observer())?;
            let stream = handler(self.ctx.clone(), req).await?;
            pump_responses(&self.ctx, &self.sender, &encode, stream).await
        }
        .await;
        self.finish(result).await;
    }

    /// Runs a client-streaming handler.
    pub async fn client_streaming<Req, Resp, D, E, F, Fut>(
        mut self,
        decode: D,
        encode: E,
        handler: F,
    ) where
        Req: Send + 'static,
        Resp: Send,
        D: Fn(&[u8]) -> Result<Req, DecodeError> + Send + Sync + 'static,
        E: Fn(&Resp) -> Vec<u8> + Send + Sync,
        F: FnOnce(RpcContext, BoxStream<'static, Result<Req, RpcError>>) -> Fut + Send,
        Fut: Future<Output = Result<Resp, RpcError>> + Send,
    {
        self.sender.observer().set_streaming("client_streaming");
        let requests = request_stream(self.request_messages(), decode, self.sender.clone());
        let result = async {
            let resp = handler(self.ctx.clone(), requests).await?;
            self.sender.send(encode(&resp)).await
        }
        .await;
        self.finish(result).await;
    }

    /// Runs a bidirectional-streaming handler.
    pub async fn bidi<Req, Resp, D, E, F, Fut>(mut self, decode: D, encode: E, handler: F)
    where
        Req: Send + 'static,
        Resp: Send + 'static,
        D: Fn(&[u8]) -> Result<Req, DecodeError> + Send + Sync + 'static,
        E: Fn(&Resp) -> Vec<u8> + Send + Sync,
        F: FnOnce(RpcContext, BoxStream<'static, Result<Req, RpcError>>) -> Fut + Send,
        Fut: Future<Output = Result<BoxStream<'static, Result<Resp, RpcError>>, RpcError>> + Send,
    {
        self.sender.observer().set_streaming("bidi_streaming");
        let requests = request_stream(self.request_messages(), decode, self.sender.clone());
        let result = async {
            let stream = handler(self.ctx.clone(), requests).await?;
            pump_responses(&self.ctx, &self.sender, &encode, stream).await
        }
        .await;
        self.finish(result).await;
    }
}

fn decode_request<Req>(
    decode: &impl Fn(&[u8]) -> Result<Req, DecodeError>,
    bytes: &[u8],
    obs: &CallObserver,
) -> Result<Req, RpcError> {
    obs.message_received(bytes.len());
    decode(bytes).map_err(|e| {
        obs.decode_failure();
        RpcError::invalid_argument(format!("invalid request message: {e}")).finish()
    })
}

fn request_stream<Req, D>(
    incoming: RequestStream,
    decode: D,
    sender: ResponseSender,
) -> BoxStream<'static, Result<Req, RpcError>>
where
    Req: Send + 'static,
    D: Fn(&[u8]) -> Result<Req, DecodeError> + Send + Sync + 'static,
{
    incoming
        .map(move |bytes| decode_request(&decode, &bytes, sender.observer()))
        .boxed()
}

/// Forwards a handler's response stream to the client; stops on the first
/// error (which becomes the final status) or when the call is cancelled.
async fn pump_responses<Resp, E>(
    ctx: &RpcContext,
    sender: &ResponseSender,
    encode: &E,
    mut stream: BoxStream<'static, Result<Resp, RpcError>>,
) -> Result<(), RpcError>
where
    E: Fn(&Resp) -> Vec<u8>,
{
    loop {
        let next = tokio::select! {
            n = stream.next() => n,
            _ = ctx.cancellation().wait_cancelled() => {
                return Err(RpcError::cancelled("call cancelled").finish());
            }
        };
        match next {
            None => return Ok(()),
            Some(Err(e)) => return Err(e),
            Some(Ok(msg)) => sender.send(encode(&msg)).await?,
        }
    }
}

/// Routes calls to registered services.
#[derive(Default)]
pub struct Server {
    services: HashMap<String, Arc<dyn Service>>,
}

impl Server {
    /// Creates an empty server.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a service under its [`Service::name`].
    pub fn add_service(mut self, service: impl Service) -> Self {
        self.services
            .insert(service.name().to_string(), Arc::new(service));
        self
    }

    /// Handles one transport call to completion.
    pub async fn handle_call(&self, call: impl IncomingCall) {
        let parts = call.into_parts();
        let cancel = CancellationToken::new();
        let first = |key: &str| parts.metadata.get(key).and_then(|v| v.first());
        let client_timeout = first(wire::TIMEOUT_KEY).and_then(|v| wire::decode_timeout(v));
        let trace = first(wire::TRACEPARENT_KEY).and_then(|v| wire::decode_traceparent(v));
        let obs = CallObserver::start(
            Side::Server,
            &parts.method,
            None,
            CallInfo {
                request_id: trace.as_ref().map(|t| t.trace_id.clone()),
                peer: None,
                deadline: client_timeout,
            },
        );
        let client_gone = parts.sender.closed_signal();
        let sender = ResponseSender::new(parts.sender, cancel.clone(), obs);

        let mut ctx = match wire::from_wire_metadata(&parts.metadata) {
            Ok(md) => RpcContext::new().with_metadata(md),
            Err(e) => return sender.finish(Err(e)).await,
        }
        .with_cancellation(cancel.clone());
        if let Some(mut t) = trace {
            if let Some(state) = first(wire::TRACESTATE_KEY) {
                t.trace_state = state.clone();
            }
            ctx = ctx.with_trace(t);
        }
        ctx = ctx.with_deadline(Deadline::from_now(
            client_timeout.unwrap_or(Duration::from_secs(365 * 24 * 3600)),
        ));

        let Some((service_name, method)) = parts.method.split_once('/') else {
            return sender
                .finish(Err(RpcError::unimplemented(format!(
                    "malformed method path `{}`",
                    parts.method
                ))
                .finish()))
                .await;
        };
        let Some(service) = self.services.get(service_name).cloned() else {
            return sender
                .finish(Err(RpcError::unimplemented(format!(
                    "unknown service `{service_name}`"
                ))
                .finish()))
                .await;
        };

        let server_call = ServerCall {
            ctx,
            request: parts.request,
            request_present: parts.request_present,
            incoming: parts.incoming,
            sender: sender.clone(),
        };
        let handling = AssertUnwindSafe(service.handle(method, server_call)).catch_unwind();
        let run = async {
            match client_timeout {
                Some(d) => tokio::time::timeout(d, handling).await.map_err(|_| ()),
                None => Ok(handling.await),
            }
        };
        // Race the handler against the client going away: the handler future
        // is dropped (at its next await) and its context cancelled, so work
        // for a caller that no longer listens does not keep running.
        tokio::select! {
            biased;
            outcome = run => match outcome {
                Err(()) => {
                    cancel.cancel();
                    sender
                        .finish(Err(
                            RpcError::deadline_exceeded("deadline exceeded").finish()
                        ))
                        .await;
                }
                Ok(Err(_panic)) => {
                    cancel.cancel();
                    sender
                        .finish(Err(RpcError::internal("handler panicked").finish()))
                        .await;
                }
                // A handler that returns without finishing is a bug; never
                // leave the client hanging or seeing an implicit success.
                Ok(Ok(())) => {
                    sender
                        .finish(Err(
                            RpcError::internal("handler did not complete the call").finish()
                        ))
                        .await;
                }
            },
            _ = client_gone => {
                cancel.cancel();
                sender
                    .finish(Err(RpcError::cancelled("client went away").finish()))
                    .await;
            }
        }
    }

    /// Serves calls arriving on an in-process transport until it closes.
    pub async fn serve_in_process(
        self: Arc<Self>,
        mut requests: tokio::sync::mpsc::Receiver<tpt20_transport::in_process::IncomingRequest>,
    ) {
        while let Some(req) = requests.recv().await {
            let server = self.clone();
            tokio::spawn(async move { server.handle_call(req).await });
        }
    }

    /// Serves calls over QUIC until the socket fails.
    #[cfg(feature = "quic")]
    pub async fn serve_quic(
        self: Arc<Self>,
        quic: &tpt20_transport::quic::QuicServer,
    ) -> Result<(), tpt20_transport::TransportError> {
        quic.serve(move |call| {
            let server = self.clone();
            Box::pin(async move {
                server.handle_call(call).await;
                Ok(())
            })
        })
        .await
    }

    /// Serves calls over HTTP/2 until the listener fails.
    #[cfg(feature = "http2")]
    pub async fn serve_http2(
        self: Arc<Self>,
        http2: &tpt20_transport::http2::Http2Server,
    ) -> Result<(), tpt20_transport::TransportError> {
        http2
            .serve(move |call| {
                let server = self.clone();
                Box::pin(async move {
                    server.handle_call(call).await;
                    Ok(())
                })
            })
            .await
    }
}

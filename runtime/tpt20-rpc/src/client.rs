//! RPC client runtime (spec §16): [`Channel`] drives calls over any
//! [`Transport`], handling metadata, deadlines, cancellation, framing of
//! typed messages and final-status decoding. Generated client stubs are thin
//! wrappers over these methods.

use crate::context::RpcContext;
use crate::error::RpcError;
use crate::observe::{CallInfo, CallObserver, Side};
use crate::status::Status;
use crate::wire;
use futures::stream::BoxStream;
use futures::{SinkExt, StreamExt};
use std::future::Future;
use std::sync::Arc;
use tpt20_core::DecodeError;
use tpt20_transport::{Call, ResponseStream, StreamItem, StreamingType, Transport};

/// A client handle to a remote service endpoint. Cheap to clone.
#[derive(Clone)]
pub struct Channel {
    transport: Arc<dyn Transport>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel").finish_non_exhaustive()
    }
}

/// Runs `fut` but fails early on deadline expiry or cancellation.
async fn guard<T>(
    ctx: &RpcContext,
    fut: impl Future<Output = Result<T, RpcError>>,
) -> Result<T, RpcError> {
    tokio::select! {
        r = fut => r,
        _ = tokio::time::sleep(ctx.remaining_time()) => {
            Err(RpcError::deadline_exceeded("deadline exceeded").finish())
        }
        _ = ctx.cancellation().wait_cancelled() => {
            Err(RpcError::cancelled("call cancelled").finish())
        }
    }
}

fn decode_error(e: DecodeError) -> RpcError {
    RpcError::internal(format!("invalid response message: {e}")).finish()
}

/// Aborts a background task when dropped.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Channel {
    /// Creates a channel over `transport`.
    pub fn new(transport: impl Transport + 'static) -> Self {
        Channel {
            transport: Arc::new(transport),
        }
    }

    /// Creates a channel over a shared transport.
    pub fn from_shared(transport: Arc<dyn Transport>) -> Self {
        Channel { transport }
    }

    async fn start(
        &self,
        method: &str,
        ctx: &RpcContext,
        first: Vec<u8>,
        streaming: StreamingType,
        obs: &CallObserver,
    ) -> Result<Call, RpcError> {
        if ctx.cancellation().is_cancelled() {
            return Err(RpcError::cancelled("call cancelled before start").finish());
        }
        if ctx.is_expired() {
            return Err(RpcError::deadline_exceeded("deadline already expired").finish());
        }
        let mut md = wire::to_wire_metadata(ctx.metadata());
        md.insert(
            wire::TIMEOUT_KEY,
            wire::encode_timeout(ctx.remaining_time()),
        );
        if let Some(tp) = wire::encode_traceparent(ctx.trace()) {
            md.insert(wire::TRACEPARENT_KEY, tp);
            if !ctx.trace().trace_state.is_empty() {
                md.insert(wire::TRACESTATE_KEY, ctx.trace().trace_state.clone());
            }
        }
        self.transport
            .start_call(method, first, &md, streaming)
            .await
            .map_err(|e| {
                obs.transport_error(&e);
                wire::transport_error(e)
            })
    }

    fn observe(method: &str, ctx: &RpcContext, streaming: &'static str) -> CallObserver {
        CallObserver::start(
            Side::Client,
            method,
            Some(streaming),
            CallInfo {
                request_id: Some(ctx.trace().trace_id.clone()).filter(|id| !id.is_empty()),
                peer: ctx.peer().map(|p| format!("{}:{}", p.addr, p.port)),
                deadline: Some(ctx.remaining_time()),
            },
        )
    }

    /// Unary call: one request, one response.
    pub async fn unary<Resp, D>(
        &self,
        method: &str,
        ctx: &RpcContext,
        request: Vec<u8>,
        decode: D,
    ) -> Result<Resp, RpcError>
    where
        D: Fn(&[u8]) -> Result<Resp, DecodeError>,
    {
        let obs = Self::observe(method, ctx, "unary");
        obs.message_sent(request.len());
        let result = guard(ctx, async {
            let call = self
                .start(method, ctx, request, StreamingType::Unary, &obs)
                .await?;
            read_single(call.stream, &decode, &obs).await
        })
        .await;
        obs.finish_result(&result);
        result
    }

    /// Server-streaming call: one request, a stream of responses.
    pub async fn server_streaming<Resp, D>(
        &self,
        method: &str,
        ctx: &RpcContext,
        request: Vec<u8>,
        decode: D,
    ) -> Result<BoxStream<'static, Result<Resp, RpcError>>, RpcError>
    where
        Resp: Send + 'static,
        D: Fn(&[u8]) -> Result<Resp, DecodeError> + Send + 'static,
    {
        let obs = Self::observe(method, ctx, "server_streaming");
        obs.message_sent(request.len());
        let started = guard(
            ctx,
            self.start(method, ctx, request, StreamingType::ServerStream, &obs),
        )
        .await;
        match started {
            Ok(call) => Ok(response_stream(call.stream, ctx.clone(), decode, None, obs)),
            Err(e) => {
                obs.finish_result::<()>(&Err(e.clone()));
                Err(e)
            }
        }
    }

    /// Client-streaming call: a stream of requests, one response.
    ///
    /// The transport's initial message slot carries an empty preamble; all
    /// real messages travel through the request stream, so an empty stream
    /// is a valid call.
    pub async fn client_streaming<Resp, D>(
        &self,
        method: &str,
        ctx: &RpcContext,
        mut requests: BoxStream<'static, Vec<u8>>,
        decode: D,
    ) -> Result<Resp, RpcError>
    where
        D: Fn(&[u8]) -> Result<Resp, DecodeError>,
    {
        let obs = Self::observe(method, ctx, "client_streaming");
        let result = guard(ctx, async {
            let call = self
                .start(method, ctx, Vec::new(), StreamingType::ClientStream, &obs)
                .await?;
            let Call { mut sink, stream } = call;
            while let Some(msg) = requests.next().await {
                let len = msg.len();
                sink.send(msg).await.map_err(|e| {
                    obs.transport_error(&e);
                    wire::transport_error(e)
                })?;
                obs.message_sent(len);
            }
            sink.close().await.map_err(wire::transport_error)?;
            drop(sink);
            read_single(stream, &decode, &obs).await
        })
        .await;
        obs.finish_result(&result);
        result
    }

    /// Bidirectional streaming call.
    pub async fn bidi<Resp, D>(
        &self,
        method: &str,
        ctx: &RpcContext,
        mut requests: BoxStream<'static, Vec<u8>>,
        decode: D,
    ) -> Result<BoxStream<'static, Result<Resp, RpcError>>, RpcError>
    where
        Resp: Send + 'static,
        D: Fn(&[u8]) -> Result<Resp, DecodeError> + Send + 'static,
    {
        let obs = Self::observe(method, ctx, "bidi_streaming");
        let started = guard(
            ctx,
            self.start(method, ctx, Vec::new(), StreamingType::Bidi, &obs),
        )
        .await;
        let call = match started {
            Ok(call) => call,
            Err(e) => {
                obs.finish_result::<()>(&Err(e.clone()));
                return Err(e);
            }
        };
        let Call { mut sink, stream } = call;
        let cancel = ctx.cancellation().clone();
        let pump_obs = obs.clone();
        let pump = tokio::spawn(async move {
            loop {
                let next = tokio::select! {
                    n = requests.next() => n,
                    _ = cancel.wait_cancelled() => None,
                };
                let Some(msg) = next else { break };
                let len = msg.len();
                if let Err(e) = sink.send(msg).await {
                    pump_obs.transport_error(&e);
                    return;
                }
                pump_obs.message_sent(len);
            }
            let _ = sink.close().await;
        });
        Ok(response_stream(
            stream,
            ctx.clone(),
            decode,
            Some(AbortOnDrop(pump)),
            obs,
        ))
    }
}

/// Reads exactly one response message followed by an OK status.
async fn read_single<Resp>(
    mut stream: ResponseStream,
    decode: &impl Fn(&[u8]) -> Result<Resp, DecodeError>,
    obs: &CallObserver,
) -> Result<Resp, RpcError> {
    let mut response = None;
    while let Some(item) = stream.next().await {
        match item.map_err(|e| {
            obs.transport_error(&e);
            wire::transport_error(e)
        })? {
            StreamItem::Message(bytes) => {
                obs.message_received(bytes.len());
                if response.is_some() {
                    return Err(
                        RpcError::internal("multiple responses to a single-response call").finish(),
                    );
                }
                response = Some(decode(&bytes).map_err(|e| {
                    obs.decode_failure();
                    decode_error(e)
                })?);
            }
            StreamItem::Trailer(trailers) => {
                wire::parse_status(&trailers)?;
                return response.ok_or_else(|| {
                    RpcError::internal("call succeeded without a response message").finish()
                });
            }
        }
    }
    Err(RpcError::unavailable("connection ended without trailers").finish())
}

/// Adapts a transport response stream into typed results, ending after the
/// trailers (an error status becomes the final `Err` item). The observer is
/// finished with the call's outcome when the stream ends.
fn response_stream<Resp, D>(
    stream: ResponseStream,
    ctx: RpcContext,
    decode: D,
    pump: Option<AbortOnDrop>,
    obs: CallObserver,
) -> BoxStream<'static, Result<Resp, RpcError>>
where
    Resp: Send + 'static,
    D: Fn(&[u8]) -> Result<Resp, DecodeError> + Send + 'static,
{
    struct State<D> {
        stream: ResponseStream,
        ctx: RpcContext,
        decode: D,
        done: bool,
        obs: CallObserver,
        _pump: Option<AbortOnDrop>,
    }
    let state = State {
        stream,
        ctx,
        decode,
        done: false,
        obs,
        _pump: pump,
    };
    futures::stream::unfold(state, |mut st| async move {
        if st.done {
            return None;
        }
        let next = guard(&st.ctx, async { Ok(st.stream.next().await) }).await;
        let item = match next {
            Err(e) => {
                st.done = true;
                st.obs.finish(e.status(), e.message());
                return Some((Err(e), st));
            }
            Ok(item) => item,
        };
        match item {
            Some(Ok(StreamItem::Message(bytes))) => {
                st.obs.message_received(bytes.len());
                let decoded = (st.decode)(&bytes).map_err(decode_error);
                if let Err(e) = &decoded {
                    st.done = true;
                    st.obs.decode_failure();
                    st.obs.finish(e.status(), e.message());
                }
                Some((decoded, st))
            }
            Some(Ok(StreamItem::Trailer(trailers))) => {
                st.done = true;
                match wire::parse_status(&trailers) {
                    Ok(()) => {
                        st.obs.finish(Status::Ok, "");
                        None
                    }
                    Err(e) => {
                        st.obs.finish(e.status(), e.message());
                        Some((Err(e), st))
                    }
                }
            }
            Some(Err(e)) => {
                st.done = true;
                st.obs.transport_error(&e);
                let e = wire::transport_error(e);
                st.obs.finish(e.status(), e.message());
                Some((Err(e), st))
            }
            None => {
                st.done = true;
                let e = RpcError::unavailable("connection ended without trailers").finish();
                st.obs.finish(e.status(), e.message());
                Some((Err(e), st))
            }
        }
    })
    .boxed()
}

//! RPC client runtime (spec §16): [`Channel`] drives calls over any
//! [`Transport`], handling metadata, deadlines, cancellation, framing of
//! typed messages and final-status decoding. Generated client stubs are thin
//! wrappers over these methods.

use crate::context::RpcContext;
use crate::error::RpcError;
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
        self.transport
            .start_call(method, first, &md, streaming)
            .await
            .map_err(wire::transport_error)
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
        guard(ctx, async {
            let call = self
                .start(method, ctx, request, StreamingType::Unary)
                .await?;
            read_single(call.stream, &decode).await
        })
        .await
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
        let call = guard(
            ctx,
            self.start(method, ctx, request, StreamingType::ServerStream),
        )
        .await?;
        Ok(response_stream(call.stream, ctx.clone(), decode, None))
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
        guard(ctx, async {
            let call = self
                .start(method, ctx, Vec::new(), StreamingType::ClientStream)
                .await?;
            let Call { mut sink, stream } = call;
            while let Some(msg) = requests.next().await {
                sink.send(msg).await.map_err(wire::transport_error)?;
            }
            sink.close().await.map_err(wire::transport_error)?;
            drop(sink);
            read_single(stream, &decode).await
        })
        .await
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
        let call = guard(
            ctx,
            self.start(method, ctx, Vec::new(), StreamingType::Bidi),
        )
        .await?;
        let Call { mut sink, stream } = call;
        let cancel = ctx.cancellation().clone();
        let pump = tokio::spawn(async move {
            loop {
                let next = tokio::select! {
                    n = requests.next() => n,
                    _ = cancel.wait_cancelled() => None,
                };
                let Some(msg) = next else { break };
                if sink.send(msg).await.is_err() {
                    return;
                }
            }
            let _ = sink.close().await;
        });
        Ok(response_stream(
            stream,
            ctx.clone(),
            decode,
            Some(AbortOnDrop(pump)),
        ))
    }
}

/// Reads exactly one response message followed by an OK status.
async fn read_single<Resp>(
    mut stream: ResponseStream,
    decode: &impl Fn(&[u8]) -> Result<Resp, DecodeError>,
) -> Result<Resp, RpcError> {
    let mut response = None;
    while let Some(item) = stream.next().await {
        match item.map_err(wire::transport_error)? {
            StreamItem::Message(bytes) => {
                if response.is_some() {
                    return Err(
                        RpcError::internal("multiple responses to a single-response call").finish(),
                    );
                }
                response = Some(decode(&bytes).map_err(decode_error)?);
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
/// trailers (an error status becomes the final `Err` item).
fn response_stream<Resp, D>(
    stream: ResponseStream,
    ctx: RpcContext,
    decode: D,
    pump: Option<AbortOnDrop>,
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
        _pump: Option<AbortOnDrop>,
    }
    let state = State {
        stream,
        ctx,
        decode,
        done: false,
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
                return Some((Err(e), st));
            }
            Ok(item) => item,
        };
        match item {
            Some(Ok(StreamItem::Message(bytes))) => {
                let decoded = (st.decode)(&bytes).map_err(decode_error);
                st.done = decoded.is_err();
                Some((decoded, st))
            }
            Some(Ok(StreamItem::Trailer(trailers))) => {
                st.done = true;
                match wire::parse_status(&trailers) {
                    Ok(()) => None,
                    Err(e) => Some((Err(e), st)),
                }
            }
            Some(Err(e)) => {
                st.done = true;
                Some((Err(wire::transport_error(e)), st))
            }
            None => {
                st.done = true;
                Some((
                    Err(RpcError::unavailable("connection ended without trailers").finish()),
                    st,
                ))
            }
        }
    })
    .boxed()
}

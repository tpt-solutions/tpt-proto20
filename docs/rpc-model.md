# RPC model

`tpt-proto20`'s RPC system is split across two crates:

- **`tpt20-rpc`** — transport-agnostic call semantics: context, streaming
  abstractions, status codes, rich errors, metadata, deadlines, cancellation,
  auth hooks, retry policy.
- **`tpt20-transport`** — the transports themselves (in-process, HTTP/2) and
  the message framing they share.

Service *shape* (unary/streaming, request/response types) comes from the
schema — see [Schema language § Services](schema-language.md#services).

## RpcContext

Every call carries an `RpcContext` (spec §16.1):

```rust
use tpt20_rpc::{RpcContext, Deadline, PeerInfo};
use std::time::Duration;

let ctx = RpcContext::new()
    .with_deadline(Deadline::from_now(Duration::from_secs(5)))
    .with_peer(PeerInfo::new("127.0.0.1", 9090));

if ctx.is_expired() {
    // reject the call
}
let remaining = ctx.remaining_time();
let md = ctx.metadata();
let trace = ctx.trace();
```

`RpcContext` is built with a chained `with_*` builder API and exposes
accessors for each part: `deadline()`, `cancellation()`, `metadata()` /
`metadata_mut()`, `trace()`, `peer()`, `extensions()` / `extensions_mut()`.
`extensions` is an open, typed bag (`Extensions`) for handler-specific
context that doesn't belong in the core struct.

## Streaming

The four RPC shapes from the schema map onto handler signatures built from
two trait objects:

```rust
pub type ServerStreamSink<T> = Box<dyn TrySink<Item = T, Error = SendError> + Send + Sync>;
pub type ClientStreamSource<T> = Box<dyn TryStream<Item = T, Error = ReceiveError> + Send + Sync>;

pub struct BidiStream<T> {
    pub sink: ServerStreamSink<T>,
    pub source: ClientStreamSource<T>,
}
```

`TryStream`/`TrySink` mirror `futures::Stream`/`Sink` but with a fixed error
type per direction, and both are poll-based (`poll_next`, `poll_ready` /
`start_send` / `poll_flush`) so a handler can only push once the sink signals
readiness — this is what makes every streaming mode backpressure-aware end to
end, from the handler down through the transport.

| Schema shape | Handler receives | Handler returns |
|---|---|---|
| unary | request | response |
| server streaming | request | writes into a `ServerStreamSink<Response>` |
| client streaming | reads from a `ClientStreamSource<Request>` | response |
| bidirectional | a `BidiStream<Message>` | — |

> Rust code generation for server traits/client stubs from `service` schema
> blocks (spec §12.6) is not implemented yet — see
> [Code generation § Services](code-generation.md#services-not-yet-generated).
> The types above exist and are usable today when wiring handlers by hand
> against `tpt20-rpc` and `tpt20-transport` directly.

## Status codes

`tpt20_rpc::Status` defines all 17 codes from spec §16.3 as a
`#[repr(i32)]` enum matching gRPC's numbering exactly (`Ok = 0` through
`Unauthenticated = 16`), with `Status::code()`, `Status::as_str()`, and
`TryFrom<i32>` for round-tripping the numeric form. This shared numbering is
what makes the [gRPC status mapping](compatibility-adapters.md#status-mapping)
a straight pass-through rather than a translation table.

## Rich errors

`RpcError` carries a `Status`, a message, and a list of structured
`ErrorDetail`s (from `tpt20-stdlib`), built through a per-status builder:

```rust
use tpt20_rpc::RpcError;
use tpt20_stdlib::ErrorDetail;

let err = RpcError::invalid_argument("validation failed")
    .with_details(ErrorDetail::new(
        "validation".into(),
        "email invalid".into(),
        Vec::new(),
    ))
    .finish();

assert_eq!(err.status(), tpt20_rpc::Status::InvalidArgument);
```

A builder method exists for every status (`RpcError::not_found(..)`,
`RpcError::unavailable(..)`, etc.); `.with_details(..)` can be chained
repeatedly before `.finish()`. Because `ErrorDetail` is a descriptor-backed
stdlib type, detail payloads can be decoded generically by any client that
has the schema — including one using [dynamic messages](code-generation.md)
with no generated types at all.

## Metadata

`Metadata` (spec §16.5) is a case-checked key/value map with an enforced
total size budget:

```rust
use tpt20_rpc::Metadata;

let mut md = Metadata::with_default_limit(); // 8192-byte budget
md.insert_text("x-request-id", "abc-123")?;      // keys must already be lowercase
md.insert_binary("x-trace-bin", b"...")?;         // binary values require a "-bin" key suffix
```

Rules enforced by `MetadataKey::new` / `Metadata::insert*`:

- keys must be lowercase (`MetadataError::KeyNotLowercase` otherwise)
- a binary value's key must end in `-bin` (`MetadataError::BinaryKeySuffixMissing`)
- inserting past the configured size budget returns `MetadataError::SizeLimitExceeded { limit }`
- lookups (`get`, `contains_key`) are case-insensitive as a convenience even
  though inserted keys must be lowercase

## Deadlines and cancellation

`Deadline::from_now(duration)` fixes an absolute expiry; `is_expired()` and
`remaining_time()` read it against `Instant::now()`. `CancellationToken` is a
shareable, settable flag (`cancel()`, `is_cancelled()`) that a handler can
poll cooperatively, or construct pre-cancelled with
`CancellationToken::cancelled()` for tests.

## Auth and authorization

`tpt20-rpc` defines both hook traits and ready-to-use implementations:

- `Authenticator` — `TokenAuthenticator` (validate a bearer token with a
  `fn(&str) -> bool`) and `MetadataAuthenticator` (require specific metadata
  keys, each with its own validator).
- `Authorizer` — `AllowAllAuthorizer`, `DenyAllAuthorizer`, `AclAuthorizer`
  (role list), `RoleBasedAuthorizer` (roles read from a configurable metadata
  key).

Both traits produce an `AuthContext` (identity + metadata) or an
`AuthError`/`AuthzError` describing why access was refused. These compose
into middleware around a handler; the RPC layer itself does not mandate a
specific pipeline shape.

## Retries and compression

`RetryPolicy::is_retryable(status)` decides whether a given `Status` should
be retried, and `backoff_for_attempt(n)` computes the delay before the next
attempt. `CompressionAlgorithm` enumerates the negotiated compression scheme
for a call (validated against `UnknownCompressionAlgorithm` on parse).

## Transport and framing

`tpt20-transport::Transport` is the single trait the RPC layer depends on:

```rust
#[async_trait]
pub trait Transport: Send + Sync {
    async fn start_call(
        &self,
        method: &str,
        request: Vec<u8>,
        metadata: &Metadata,
        streaming_type: StreamingType,
    ) -> Result<Call, TransportError>;
}
```

`Call` bundles a `Sink<Vec<u8>>` for outgoing messages with a
`Stream<Item = Result<StreamItem, TransportError>>` for incoming ones, where
`StreamItem` is either a `Message(Vec<u8>)` or a final `Trailer(Metadata)`.

Every message on the wire — regardless of transport — is framed:

```text
1 byte  flags   (bit 0: compressed; bits 1-7: reserved, must be 0)
4 bytes length  (big-endian, payload length in bytes)
N bytes payload
```

`tpt20_transport::{encode_frame, decode_frame}` implement this; a frame with
any reserved bit set is rejected.

### In-process transport

`InProcessServer` / `InProcessTransport` provide a transport that never
touches a socket, for tests, embedded use, and fuzzing. `InProcessServer::bind(capacity)`
returns a server handle plus an `mpsc::Receiver<IncomingRequest>` the
application drains to service calls.

### HTTP/2 transport

`Http2Transport` / `Http2Server` (feature `http2`, built on the `h2` crate)
provide the production transport, with `Endpoint`/`TlsConfig` for connection
setup (`Endpoint::http2()`, `.with_tls(..)`, `.with_pem_paths(..)`,
`.require_client_cert(true)` for mTLS, `.with_max_message_bytes(..)`).

**Known gaps against spec §17.1**, tracked in `todo.md` Phase 11 — check
there before depending on any of these:

- the `tls` feature currently **does not compile**: it targets a `rustls`
  API newer than the pinned dependency version
- the client fabricates empty trailers instead of reading the server's real
  ones
- `TransportError::StreamReset` and `TransportError::GoAway` are defined but
  never produced
- keepalive/ping is not actually configured on the `h2` builders despite doc
  comments claiming it

QUIC/HTTP3 is an empty feature flag with no implementation yet.

## Observability

Every RPC surface above is designed to be instrumented by
[`tpt20-observability`](observability.md) — see that guide for the metrics,
tracing attributes, and log fields the runtime is expected to emit at each
call boundary.

# Security limits

`tpt-proto20` assumes every message it decodes may come from a hostile
sender (spec §18). This guide covers the mechanisms enforced today and
flags the ones described in the design that are not wired up yet.

## Decoder limits

Every decode path in `tpt20-core` is bounded by a `DecoderLimits` value:

```rust
pub struct DecoderLimits {
    pub max_message_bytes: usize,
    pub max_depth: usize,
    pub max_field_count: usize,
    pub max_unknown_field_bytes: usize,
    pub max_string_bytes: usize,
    pub max_bytes_field_bytes: usize,
    pub max_repeated_entries: usize,
    pub max_map_entries: usize,
}
```

`DecoderLimits::default()` is conservative but non-zero:

| Limit | Default |
|---|---:|
| `max_message_bytes` | 4 MiB |
| `max_depth` | 100 |
| `max_field_count` | 32,768 |
| `max_unknown_field_bytes` | 4 MiB |
| `max_string_bytes` | 4 MiB |
| `max_bytes_field_bytes` | 16 MiB |
| `max_repeated_entries` | 524,288 |
| `max_map_entries` | 524,288 |

Every generated `decode` method calls `decode_with_limits` internally with
these defaults; pass an explicit `DecoderLimits` (via `decode_with_limits`)
to tighten them for a specific untrusted boundary — e.g. a public HTTP
gateway vs. an internal service mesh.

Exceeding a limit produces a specific `DecodeError` rather than an opaque
failure, so callers can distinguish "hostile/oversized input" from
"malformed input":

| Condition | Error |
|---|---|
| total message size | `LimitExceeded { limit }` |
| nesting depth (`max_depth`) | `DepthExceeded` |
| field count per message | `FieldCountExceeded` |
| a single `string`/`bytes` field | `LimitExceeded { limit }` |
| repeated field entry count | `RepeatedEntriesExceeded` |
| map entry count | `MapEntriesExceeded` |

Depth is checked recursively on every nested message decode
(`DecoderLimits::check_depth`), so a deeply nested — or self-referential via
repeated fields — payload is rejected before it can exhaust the stack, and
before large allocations are made for its contents.

## Other decode-path protections

- **Varint safety** — overlong (>10-byte) and overflowing varints are
  rejected (`DecodeError::VarintOverflow`) rather than silently wrapping; see
  [Wire format § Varints](wire-format.md#varints).
- **Integer safety** — length and offset arithmetic throughout the codec
  uses checked operations; there is no path where a crafted length can wrap
  an unsigned integer into a small value that bypasses a size check.
- **UTF-8 validation** — every `string` field is validated on decode
  (`DecodeError::InvalidUtf8`); invalid bytes never reach application code as
  a `String`.
- **Unknown field policy** — see
  [Wire format § Unknown fields](wire-format.md#unknown-fields). `Fail`
  policy turns "message contains fields I don't recognize" into a hard
  decode error for callers who want strict-schema semantics.
- **No `unsafe` in decoding paths** — spec §18.7's default policy holds
  throughout `tpt20-core`. Any future exception must be isolated,
  documented, tested, feature-gated, and independently justified — none
  exist today.

## RPC-level protections

These are covered in detail in [RPC model](rpc-model.md) and
[Compatibility adapters](compatibility-adapters.md):

- **TLS / mTLS** — `Endpoint`/`TlsConfig` in `tpt20-transport` support
  server TLS and, via `require_client_cert(true)`, mutual TLS. **The `tls`
  Cargo feature does not currently compile** (see
  [RPC model § HTTP/2 transport](rpc-model.md#http2-transport)) — it targets
  a `rustls` API newer than the pinned dependency. Treat TLS as
  not-yet-usable until that's fixed, not as a hardened default.
- **Authentication** — `Authenticator` implementations (`TokenAuthenticator`,
  `MetadataAuthenticator`) validate bearer tokens or required metadata
  before a handler runs.
- **Authorization** — `Authorizer` implementations (`AclAuthorizer`,
  `RoleBasedAuthorizer`, plus `AllowAllAuthorizer`/`DenyAllAuthorizer`) gate
  access after authentication.
- **Peer inspection** — `RpcContext::peer()` exposes `PeerInfo` to handlers
  and middleware that need to make address-based decisions.
- **Deadline enforcement** — `RpcContext::is_expired()` /
  `remaining_time()` let a handler bail out of expensive work once a
  caller's deadline has passed; see [RPC model § Deadlines](rpc-model.md#deadlines-and-cancellation).
- **Metadata size limits** — `Metadata::with_default_limit()` caps total
  metadata at 8 KiB per call and rejects malformed keys (see
  [RPC model § Metadata](rpc-model.md#metadata)).

### Rate limiting: current gap

Spec §18.6 calls for rate-limiting hooks. `runtime/tpt20-rpc/src/limits.rs`
contains a `RateLimiter` trait, `TokenBucketRateLimiter`,
`CompositeRateLimiter`, and a `RequestLimits` struct (message/header/metadata
size caps distinct from the codec's `DecoderLimits`) — but **this module is
not declared in `tpt20-rpc`'s `lib.rs`** (no `pub mod limits;`), so none of
it is reachable from outside the crate today, and as written it calls
`RpcError`/`Metadata` APIs (`RpcError::Status(..)`, `Metadata::total_bytes()`)
that don't match the current shape of those types — it would not compile if
wired in as-is. Don't rely on it; if you need request-level rate limiting or
size caps today, enforce them in your own middleware ahead of the handler,
using `DecoderLimits` for payload-size bounds and your own counter for
request-rate bounds.

## Testing hostile input

The conformance and fuzz suites (`tools/tpt20-conformance`, `fuzz/`) are the
mechanism for validating limit enforcement end to end — see spec §22.3 and
`todo.md` Phase 17 for current coverage and known gaps (some fuzz targets are
mislabeled or not yet wired to the code paths their names suggest). At the
time of writing, targets exist for: the binary decoder, the schema parser,
the descriptor decoder, the dynamic message decoder, RPC framing, and
metadata parsing. Security-limit conformance (decoder limits actually
rejecting oversized/deep/malicious input) is covered by the native
conformance suite.

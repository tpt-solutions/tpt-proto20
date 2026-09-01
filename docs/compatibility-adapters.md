# Compatibility adapters

`tpt-proto20` provides two adapter crates so existing protobuf/gRPC systems
can migrate gradually rather than all at once (spec §10):

```text
existing protobuf/gRPC system -> adapter -> gradual migration -> native tpt20 usage
```

- **`tpt20-compat-protobuf`** — import `.proto` schemas as `.tpt`/IR, and
  encode/decode protobuf-wire-compatible binary messages.
- **`tpt20-compat-grpc`** — gRPC-compatible framing, status/metadata/deadline
  mapping, streaming-mode mapping, and health checking.

## Protobuf schema import

```rust
let src = std::fs::read_to_string("user.proto")?;
let tokens = tpt20_compat_protobuf::lex_proto(&src)?;
let proto_ast = tpt20_compat_protobuf::parse_proto(tokens)?;
let ir = tpt20_compat_protobuf::lower(proto_ast)?;
```

or from the CLI:

```sh
tpt20 import-proto user.proto --out user.tpt
```

(the CLI currently prints/writes the lowered IR as JSON rather than
re-serializing `.tpt` source text — see
[CLI reference § import-proto](cli-reference.md#import-proto).)

Supported: proto2, proto3, messages, enums, oneofs, maps, services, options
where meaningful, and message-level `reserved` fields.

Not yet supported:

- **Editions** — doc comments describe editions support, but there is no
  `edition = "..."` lexing/parsing implemented.
- **`extend` blocks** — parsed but discarded; nothing is lowered into IR.
- **Enum-level `reserved`** — parsed but not yet stored/lowered (message-level
  `reserved` works).

## Protobuf wire adapter

```rust
let bytes = b"\x08\x96\x01"; // field 1, varint 150
let msg = tpt20_compat_protobuf::wire::decode_protobuf(bytes)?;
let encoded = tpt20_compat_protobuf::wire::encode_protobuf(&msg)?;
```

`decode_protobuf`/`encode_protobuf` (plus `_with` variants taking explicit
options) read and write the standard protobuf wire encoding — which shares
its tag scheme and wire classes with the native tpt20 format (see
[Wire format](wire-format.md)), so the adapter is largely a thin, focused
layer rather than a second codec.

This lets a message defined once be moved between an existing protobuf
service and a tpt20 one during migration without re-encoding through JSON or
another intermediate format.

**Testing gap:** round-trip fidelity has only been tested by comparing
tpt20's protobuf encoder against its own decoder (self-consistency). There is
no dependency on a reference protobuf implementation (e.g. `prost`) yet, so
there is no golden-vector or differential test against real-world protobuf
output. Don't treat this adapter as validated against arbitrary third-party
protobuf messages until that testing exists.

## gRPC bridge

`tpt20-compat-grpc` translates between tpt20's RPC types and gRPC's wire
conventions:

```rust
pub use status::{from_grpc_status, grpc_status_name, to_grpc_status};
pub use metadata::{from_grpc_headers, from_grpc_trailers, to_grpc_headers, to_grpc_trailers};
pub use deadline::{parse_grpc_timeout, format_grpc_timeout};
pub use streaming::{from_grpc_streaming, to_grpc_streaming, GrpcStreamingType};
pub use frame::{decode_grpc_frame, encode_grpc_frame, grpc_frame_len};
pub use health::{HealthHandler, HealthRegistry, ServingStatus};
pub use reflection::ReflectionService;
pub use server::GrpcServer;
pub use client::GrpcClient;
```

### Status mapping

Because `tpt20_rpc::Status` uses the exact same 0–16 numbering as gRPC status
codes, `to_grpc_status`/`from_grpc_status` are a direct pass-through
(`status.code()`) rather than a translation table — there is no
representational drift to worry about between the two systems.

> **Known bug (`todo.md` Phase 15):** the mapping functions themselves work
> correctly, but `GrpcClient`/`GrpcStream::poll_next` don't call them yet —
> they hardcode `Status::Ok` for every response instead of reading the
> `grpc-status`/`grpc-message` trailers. **A failed call made through
> `GrpcClient` is currently misreported as successful.** Do not rely on
> client-side status reporting from this crate until that's fixed.

### Metadata mapping

`to_grpc_headers`/`from_grpc_headers` and `to_grpc_trailers`/`from_grpc_trailers`
convert between tpt20's `Metadata` (see
[RPC model § Metadata](rpc-model.md#metadata)) and gRPC's HTTP/2
header/trailer conventions, including the `-bin` binary-value suffix both
systems share.

### Deadline mapping

`parse_grpc_timeout`/`format_grpc_timeout` convert to and from gRPC's
`grpc-timeout` header format (a value + unit suffix, e.g. `"5000m"` for 5000
milliseconds).

### Streaming mode mapping

`GrpcStreamingType` plus `to_grpc_streaming`/`from_grpc_streaming` map
tpt20's four call shapes (unary / server / client / bidi — see
[RPC model § Streaming](rpc-model.md#streaming)) onto the equivalent gRPC
streaming semantics.

### Message framing

`encode_grpc_frame`/`decode_grpc_frame` implement gRPC's 5-byte frame header
(1 compression-flag byte + 4-byte big-endian length), independent of tpt20's
own frame format (see [RPC model § Transport](rpc-model.md#transport-and-framing)) —
the two are not interchangeable, since a message crossing the bridge is
reframed, not just relabeled.

### Health checking

`HealthRegistry` tracks per-service `ServingStatus` (`Unknown`, `Serving`,
`NotServing`, `ServiceUnknown`); `HealthHandler::check(service)` answers a
`grpc.health.v1.Health/Check` request by encoding a gRPC-framed
`HealthCheckResponse`. An empty service name addresses the overall server
status, which defaults to `Serving` until explicitly set otherwise.

### Reflection

`ReflectionService` exists as a minimal in-memory symbol registry, but it is
**not yet wired to the real `grpc.reflection.v1alpha.ServerReflection` wire
service** — existing gRPC reflection clients (e.g. `grpcurl -reflect`) cannot
talk to it yet.

### Server and client status

`GrpcServer::serve()` is currently a hardcoded "not supported" stub — there
is no live network gRPC server yet, only the framing/mapping building blocks
above. `GrpcClient` can perform calls but inherits the status-mapping bug
described above. Track `todo.md` Phase 15 for progress.

## Choosing an adapter path

- Importing an existing `.proto` schema library → `tpt20-compat-protobuf`'s
  import path, followed by `tpt20 gen rust` on the resulting `.tpt`/IR.
- Talking protobuf bytes over your own transport (e.g. a message queue) →
  `tpt20-compat-protobuf::wire`.
- Exposing or consuming a gRPC service → `tpt20-compat-grpc`, with the
  caveats above; today this is most usable for the mapping/framing
  primitives, not yet as a drop-in gRPC server or a trustworthy client.

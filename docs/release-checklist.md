# v1.0 acceptance checklist (spec §27)

Status of each §27 criterion against the current tree. `[x]` = met and covered
by tests; `[ ]` = not met yet, with the gap. This is a live checklist: a box is
ticked only when the evidence named next to it exists. **v1.0 is not ready:**
see the unchecked items.

## 27.1 Schema language
- [x] messages, enums, oneofs, maps, repeated, optional presence, services,
      streaming methods, annotations, packages, imports, reserved IDs
      (`compiler/tpt20-language`, `docs/schema-language.md`)

## 27.2 Compiler
- [x] parse, semantic errors (E0001–E0017), IR, descriptors, fingerprints,
      Rust codegen (messages, enums, oneofs, views, JSON, builders, services),
      diagnostics with locations

## 27.3 Runtime
- [x] encode/decode, unknown-field preservation, `DecoderLimits`, borrowed
      decoding, dynamic decoding, JSON conversion, text conversion
      (`tpt20-text`)

## 27.4 Reflection
- [x] descriptor-driven decode/encode, field access/mutation, dynamic
      inspection (`tpt20-reflect`)

## 27.5 RPC
- [x] unary, server/client/bidi streaming, deadlines, cancellation, metadata,
      rich errors, TLS, health checking (`tpt20 health` convention)
- [x] compression (gzip/deflate over HTTP/2, negotiated, bounded decompression)
- [x] reflection — native `tpt20.reflection.v1.Reflection` service
      (`ListServices`, `GetDescriptor`) and `tpt20 reflect-remote`; the *gRPC*
      reflection wire protocol (`grpc.reflection.v1alpha`/`v1`) is served by
      `tpt20-compat-grpc`'s `reflection_wire::ReflectionServer`
- [x] observability — metrics, structured logs and W3C trace propagation are
      built into `Channel`/`Server` (`docs/observability.md`)

## 27.6 Compatibility
- [x] import protobuf schemas (proto2/proto3; no editions, extensions dropped)
- [x] encode/decode protobuf-compatible binary (self-consistency tested)
- [ ] differential testing against an established protobuf implementation
- [x] expose/consume gRPC-compatible services over the network:
      `GrpcServer::serve`/`serve_listener` (feature `server`) answer stock
      gRPC HTTP/2 clients (tested with a plain `h2` client); `GrpcClient`
      consumes; reflection wire service included.

## 27.7 Tooling
- [x] check, fmt, lint, diff, gen, decode, encode, JSON conversion, text
      conversion, RPC debugging (`call`, `health`), registry publish/list/get
- [x] conformance execution — `tpt20 conformance` runs the JSON vectors in
      `conformance/vectors` (52 cases); the Rust suite runs via `cargo test`

## 27.8 Security
- [x] limit-enforcement and malformed-input tests; fuzz targets for every
      decoder/parser (binary, JSON, text, schema, descriptor, dynamic, RPC
      framing, metadata)
- [x] malicious-schema test corpus (`compiler/tpt20-compiler/tests/hostile_schemas.rs`:
      deep nesting, huge schemas, garbage input, run on a 512 KiB stack; the
      parser caps message nesting at 64 — diagnostic E0018)
- [x] RPC abuse tests (`http2::tests` in `tpt20-transport`: connection flood cap,
      silent/garbage clients and handshake timeout, oversized metadata,
      per-connection stream cap)

## 27.9 Documentation
- [x] quickstart, schema language, wire format, RPC model, compatibility
      adapters, security limits, observability, code generation, CLI usage,
      provenance policy, performance, stability policy, governance

## Release mechanics
- [x] CI green on `master` under `-D warnings` (fmt, clippy, build, test)
- [x] Semantic versioning and stability policy documented
      ([stability.md](stability.md))
- [ ] crate metadata/`CHANGELOG.md` entries reviewed for publishing
- [ ] `0.x` → `1.0` version bump

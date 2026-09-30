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
- [ ] **compression** — frame flag exists; no compression codec or negotiation
- [ ] **reflection** — the gRPC reflection service is not wired to the real
      wire protocol (`compat/tpt20-compat-grpc`); native RPC reflection service
      does not exist (schema reflection is offline via `tpt20-reflect`)
- [ ] **observability** — `tpt20-observability` exists but is not yet wired
      into `Channel`/`Server` call boundaries

## 27.6 Compatibility
- [x] import protobuf schemas (proto2/proto3; no editions, extensions dropped)
- [x] encode/decode protobuf-compatible binary (self-consistency tested)
- [ ] differential testing against an established protobuf implementation
- [ ] expose/consume gRPC-compatible services over the network: framing,
      status/metadata mapping and `GrpcClient` exist; `GrpcServer::serve` is a
      stub

## 27.7 Tooling
- [x] check, fmt, lint, diff, gen, decode, encode, JSON conversion, text
      conversion, RPC debugging (`call`, `health`), registry publish/list/get
- [ ] **conformance execution** — `tpt20 conformance` walks JSON vectors but does
      not run the `tpt20-conformance` suite

## 27.8 Security
- [x] limit-enforcement and malformed-input tests; fuzz targets for every
      decoder/parser (binary, JSON, text, schema, descriptor, dynamic, RPC
      framing, metadata)
- [ ] malicious-schema test corpus (deeply nested / huge schemas through the
      compiler) — partial
- [ ] RPC abuse tests (connection floods, slow clients, oversized metadata) —
      size limits are tested; flood/slow-client behavior is not

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

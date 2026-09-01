# tpt-proto20

A next-generation, schema-first contract system for distributed applications.

`tpt-proto20` combines a modern schema language, a pure-Rust compiler, a
compact binary wire format, a Rust-first runtime, generated code, reflection,
JSON/text representations, schema-evolution tooling, security-hardened
decoding, observability, and an RPC/streaming system — plus compatibility
adapters for the existing Protocol Buffers and gRPC ecosystems.

The goal is to let independently evolving programs communicate safely,
efficiently, and predictably over time.

## Documentation

See [`docs/`](docs/) for in-depth guides once available, and [`spec.txt`](spec.txt)
for the full-scope design document.

## Status

Work is in active development. Multiple phases are complete; remaining phases
are tracked in [`todo.md`](todo.md).

## Workspace layout

- `compiler/tpt20-language/` — `.tpt` lexer, parser, and AST
- `compiler/tpt20-ir/` — neutral IR types
- `compiler/tpt20-descriptor/` — compiled schema descriptors
- `compiler/tpt20-compiler/` — semantic analysis, compatibility checks, IR/descriptor/codegen pipeline
- `compiler/tpt20-codegen-rust/` — Rust code generator
- `runtime/tpt20-core/` — native binary wire format, encode/decode, `DecoderLimits`
- `runtime/tpt20-reflect/` — descriptor-driven dynamic encode/decode and reflection
- `runtime/tpt20-json/` — JSON mapping for messages
- `runtime/tpt20-stdlib/` — well-known types (timestamp, duration, any, uuid, etc.)
- `runtime/tpt20-rpc/` — RPC model, streaming, status codes, metadata, security
- `runtime/tpt20-transport/` — HTTP/2, in-process, framing, metadata, endpoints
- `runtime/tpt20-observability/` — metrics, tracing, structured logging
- `compat/tpt20-compat-protobuf/` — `.proto` import and protobuf wire adapter
- `compat/tpt20-compat-grpc/` — gRPC-compatible transport and mapping
- `tools/tpt20-cli/` — developer CLI (`tpt20 check`, `tpt20 gen rust`, etc.)
- `tools/tpt20-conformance/` — native and compatibility conformance suites
- `tests/` — integration and conformance tests
- `fuzz/` — fuzz targets for decode paths, schema parsing, RPC framing, etc.
- `provenance/` — clean-room provenance and contribution policy

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).

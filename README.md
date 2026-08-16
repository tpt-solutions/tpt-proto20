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

## Workspace layout

- `compiler/` — schema language, IR, descriptor, compiler, codegen
- `runtime/` — core wire codec, reflection, JSON/text, stdlib, RPC, transport
- `compat/` — protobuf / gRPC compatibility adapters
- `tools/` — developer CLI and tooling
- `tests/`, `fuzz/`, `benches/` — conformance, fuzzing, benchmarks

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).

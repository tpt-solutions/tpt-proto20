# Changelog

All notable changes to `tpt20-cli` are documented here.

## [Unreleased]

### Added
- Initial CLI binary (Phase 16, spec §21)
- `init` — scaffold a new tpt20 project with `Cargo.toml`, schema, and `.gitignore`
- `check` — semantic-check a schema without codegen
- `fmt` — rewrite a schema in canonical form with `--check` mode
- `lint` — run configurable lint rules with JSON/text output
- `diff` — compare two schemas and render SAFE/WARNING/BREAKING report
- `gen rust` — generate Rust code from a schema with optional builders
- `descriptors` — emit compiled descriptor as JSON or binary
- `decode` / `encode` — dynamic JSON ↔ binary conversion
- `text-to-binary` / `binary-to-text` — text format conversion
- `json-to-binary` / `binary-to-json` — JSON format conversion
- `import-proto` — import a `.proto` file to `.tpt`
- `conformance` — run conformance test vectors
- `call` — RPC debugger stub (unary/streaming, metadata, deadline, TLS, compression)
- `health` — health check stub
- `reflect` — introspect a descriptor
- `registry publish` — publish a descriptor to the local registry

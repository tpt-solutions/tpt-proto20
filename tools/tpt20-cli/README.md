# tpt20-cli

`tpt20` command-line interface — Phase 16 Developer Tooling (spec §21).

## Subcommands

- `init` — create a new tpt20 project
- `check` — semantic-check a schema without codegen
- `fmt` — rewrite a schema in canonical form
- `lint` — run configurable lint rules
- `diff` — compare two schemas (SAFE/WARNING/BREAKING)
- `gen rust` — generate Rust code from a schema
- `descriptors` — emit the compiled descriptor (JSON or binary)
- `decode` — decode binary to a dynamic JSON representation
- `encode` — encode JSON to binary
- `text-to-binary` — convert text format to binary
- `binary-to-text` — convert binary to text format
- `json-to-binary` — convert JSON to binary
- `binary-to-json` — convert binary to JSON
- `import-proto` — import a .proto file to .tpt
- `conformance` — run conformance test vectors
- `call` — RPC debugger (unary/streaming)
- `health` — check service health
- `reflect` — introspect a descriptor
- `registry publish` — publish a descriptor to the local registry

## Installation

```sh
cargo install --path tools/tpt20-cli
```

# tpt-proto20 documentation

This directory documents the `tpt-proto20` contract system: schema language,
wire format, runtime, RPC, compatibility adapters, tooling, and project
policy. It satisfies the documentation acceptance criteria in `spec.txt` §27.9.

| Guide | Covers |
|---|---|
| [Quickstart](quickstart.md) | Install the CLI, write a schema, generate Rust code, encode/decode a message |
| [Schema language reference](schema-language.md) | `.tpt` syntax: messages, fields, presence, enums, oneofs, maps, services, annotations |
| [Wire format specification](wire-format.md) | The native binary encoding, tag/varint rules, canonical mode, unknown fields |
| [RPC model](rpc-model.md) | `RpcContext`, streaming, status codes, rich errors, metadata, transports |
| [Compatibility adapters](compatibility-adapters.md) | Protobuf `.proto` import/wire adapter and the gRPC bridge |
| [Security limits](security-limits.md) | `DecoderLimits`, hostile-input handling, RPC hardening |
| [Observability](observability.md) | Metrics, tracing attributes, structured logging |
| [Code generation](code-generation.md) | What `tpt20 gen rust` produces and how to use it |
| [CLI reference](cli-reference.md) | Every `tpt20` subcommand, its flags, and current limitations |
| [Provenance policy](provenance-policy.md) | Clean-room and AI-assisted contribution policy |

## Project status

`tpt-proto20` is under active development. These docs describe the system
**as implemented today**, and call out gaps or known bugs against the design
in [`spec.txt`](../spec.txt) where they exist. The authoritative, up-to-date
list of what is done, in progress, or stubbed is [`todo.md`](../todo.md) at
the repository root — consult it before relying on a feature these docs
describe as partial.

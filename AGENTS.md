# AGENTS.md — tpt-proto20

## Project

Rust workspace (edition 2021, MSRV 1.74). Schema-first contract system with compiler, runtime, and protobuf/gRPC compatibility layers. Source of truth for scope: `spec.txt`. Progress tracker: `todo.md`.

## Commands

CI order is the verified source of truth:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features
cargo build --all-targets --all-features
cargo test --all-features
```

Fuzz targets are workspace members; run a single target with:

```sh
cargo test -p fuzz/binary_decoder
```

## Clean-room and AI policy

This project uses a **clean-room process** and an **AI-assisted contribution policy** (`CONTRIBUTING.md`, spec §25).

- **Allowed inputs:** public specs, original design notes/tests, independently created test vectors.
- **Disallowed inputs:** upstream implementation source, proprietary implementations, copied codegen templates, AI prompts containing upstream source code.
- **AI-assisted changes:** must be human-reviewed, include tests, pass CI, and be documented.
- **Contributions:** issues only. Do not open pull requests.

Do not use upstream source code or training data as a substitute for original design.

## Workspace layout

- `compiler/` — language → IR → descriptor → compiler → Rust codegen
- `runtime/` — core wire format, JSON, stdlib, RPC, transport, observability, reflection
- `compat/` — protobuf and gRPC adapters
- `tools/tpt20-cli/` — developer CLI (`tpt20` binary)
- `tools/tpt20-conformance/` — conformance suites
- `tests/` — integration and codegen tests
- `fuzz/` — fuzz targets

## Key constraints

- Core decode paths: no `unsafe` by default (spec §18.7). Any exception is isolated, documented, tested, feature-gated, and justified.
- `DecoderLimits` must be enforced on every decode path.
- `UnknownFieldPolicy` controls unknown-field handling.
- Canonical encoding is deterministic (field order, map order, varint form).

## References

- `spec.txt` — full design document
- `CONTRIBUTING.md` — contribution policy and CI commands
- `todo.md` — phase-by-phase checklist

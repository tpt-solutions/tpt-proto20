# tpt20-codegen-rust

Rust code generator for tpt20 schemas (spec §12).

## Overview

Consumes the neutral IR ([`tpt20_ir::PackageIr`]) and emits a single, self-contained Rust module containing:

- Owned message structs with `encode` / `decode` / `decode_with_limits` / `encode_canonical` / `to_raw`
- Borrowed view types (`XView<'a>`) with `decode_borrowed` for zero-copy access to string/bytes payloads
- Generated enums respecting open/closed unknown-value semantics
- Oneofs as mutually exclusive Rust enums
- JSON conversion methods (`to_json` / `from_json`) per spec §14.2
- Opt-in validated builders

Generated code depends on `tpt20-core` and `tpt20-json`.

## Usage

```rust
use tpt20_codegen_rust::{generate_module, CodegenOptions};

let mut opts = CodegenOptions::default();
opts.builders = true;

let module = generate_module(&ir, &opts);
std::fs::write("src/generated/user.rs", module).expect("writes");
```

## Crate layout

- `src/emit.rs` — Rust code emission
- `src/model.rs` — codegen model construction
- `src/expr.rs` — expression helpers
- `src/naming.rs` — name generation
- `src/scalars.rs` — scalar type mapping

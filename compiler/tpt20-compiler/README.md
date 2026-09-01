# tpt20-compiler

tpt20 schema compiler: semantic analysis, compatibility checks, IR/descriptor generation, and schema-history manifest support (spec §7, §8, §20).

## Overview

Pipeline:

```
.tpt → lexer → parser → AST → semantic analysis → IR → descriptor → fingerprint
```

This crate provides:

- [`check`] — semantic-check a schema without codegen
- [`compile`] — full compilation producing [`CompileOutput`] (IR, descriptor, fingerprint, diagnostics)
- [`diff_sources`] / [`render_report`] — compare two schemas and report changes as `SAFE` / `WARNING` / `BREAKING`
- [`Diagnostic`] / [`render_all`] — structured diagnostics with file/line/column/severity/code
- [`SchemaHistoryManifest`] — schema version history with fingerprints and policies

## Usage

```rust
let src = r#"package user.v1; message User { 1: id int64; 2: name string; }"#;
let output = tpt20_compiler::compile(src, Some("user.tpt")).expect("compiles");
println!("fingerprint: {}", output.fingerprint);
```

## Crate layout

- `src/pipeline.rs` — `check` and `compile`
- `src/semantic.rs` — semantic analysis pass
- `src/compat.rs` — compatibility-change detector
- `src/diagnostics.rs` — diagnostics engine
- `src/manifest.rs` — schema history manifest
- `src/ast_to_ir.rs` — AST lowering to IR

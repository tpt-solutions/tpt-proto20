# tpt20-ir

Neutral compiler IR for tpt20 (spec §8).

## Overview

The intermediate representation is the language-agnostic, in-memory model produced after parsing and consumed by semantic analysis, code generation, and descriptor serialization.

It deliberately mirrors the AST but adds:

- [`SourceSpan`] — file/line/column locations for diagnostics
- Compatibility metadata (`CompatMetadata`)
- Stable schema fingerprint (`PackageIr::fingerprint`)

## Key types

- [`PackageIr`] — top-level compiled file
- [`MessageIr`] — message definition
- [`FieldIr`] — field definition with label, type, presence, annotations
- [`EnumIr`] — enum definition with open/closed semantics
- [`ServiceIr`] / [`MethodIr`] — service and method definitions
- [`CompatMetadata`] — compatibility policy and history

## Usage

```rust
use tpt20_ir::PackageIr;

let ir = PackageIr {
    name: Some("user.v1".into()),
    messages: vec![],
    ..Default::default()
};
```

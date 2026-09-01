# tpt20-descriptor

Descriptor model and serialization for tpt20 (spec §8).

## Overview

A [`Descriptor`] wraps the neutral [`tpt20_ir::PackageIr`] and provides:

- JSON serialization (`to_json` / `from_json`)
- Binary serialization (`to_binary` / `from_binary`) with a stable, deterministic, self-describing layout
- Dynamic lookup by name and id (consumed by reflection in Phase 7)

## Usage

```rust
use tpt20_descriptor::Descriptor;

let json = r#"{"name":"user.v1","messages":[]}"#;
let descriptor = Descriptor::from_json(json).expect("valid descriptor");

let bytes = descriptor.to_binary().expect("serialize");
let back = Descriptor::from_binary(&bytes).expect("deserialize");
```

## Crate layout

- `src/lib.rs` — `Descriptor`, `DescriptorError`, binary reader/writer
- `src/*` — field/message descriptor types

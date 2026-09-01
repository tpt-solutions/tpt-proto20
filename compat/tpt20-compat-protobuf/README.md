# tpt20-compat-protobuf

Protobuf compatibility adapter for tpt-proto20 (spec §10).

## Overview

Provides:

- `.proto` schema import (proto2, proto3, Editions) → `tpt20_ir::PackageIr`
- Protobuf wire format encode/decode adapters

## Proto schema import

```rust
let src = std::fs::read_to_string("user.proto")?;
let tokens = tpt20_compat_protobuf::lex_proto(&src)?;
let proto_ast = tpt20_compat_protobuf::parse_proto(tokens)?;
let ir = tpt20_compat_protobuf::lower(proto_ast)?;
```

## Protobuf wire adapter

```rust
let bytes = b"\x08\x96\x01"; // field 1, varint 150
let msg = tpt20_compat_protobuf::wire::decode_protobuf(bytes)?;
let encoded = tpt20_compat_protobuf::wire::encode_protobuf(&msg)?;
```

## Crate layout

- `src/lexer.rs` — proto tokenizer
- `src/parser.rs` — proto parser
- `src/lower.rs` — lowering to `tpt20_ir::PackageIr`
- `src/wire.rs` — protobuf wire format adapters
- `src/proto_ast.rs` — proto AST types

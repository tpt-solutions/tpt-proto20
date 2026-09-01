# tpt20-core

Native binary wire format and core runtime for tpt20 (spec §9, §11, §18).

## Overview

This crate provides the safe-by-default decode/encode primitives for the tpt20 native wire format. It is deliberately free of `unsafe` in the decoding paths (spec §9 policy) and uses checked arithmetic throughout.

The design targets untrusted input: every decoder limit in [`DecoderLimits`] is enforced on the decode path with conservative defaults.

## Key types

- [`RawMessage`] / [`Field`] — owned message model
- [`BorrowedMessage`] / [`BorrowedField`] — zero-copy borrowed view
- [`Value`] — scalar/bytes value enum
- [`WireClass`] — wire class constants (VARINT, FIXED32, FIXED64, LEN)
- [`Tag`] — encoded field tag
- [`DecoderLimits`] — configurable decode limits
- [`UnknownFieldPolicy`] — preserve / discard / fail
- [`DynamicMessage`] — descriptor-driven dynamic message
- [`Envelope`] — optional schema-identified wrapper

## Usage

```rust
use tpt20_core::{DecoderLimits, RawMessage, UnknownFieldPolicy, Value, WireClass};

let mut msg = RawMessage::new();
msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
let bytes = msg.encode().expect("encodes");

let decoded = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve)
    .expect("decodes");
```

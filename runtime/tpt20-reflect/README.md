# tpt20-reflect

Descriptor-driven reflection for tpt20 schemas (spec §13).

## Overview

This crate provides [`DynamicMessage`], a schema-aware dynamic message type built on top of [`tpt20_core::RawMessage`] and [`tpt20_descriptor::Descriptor`]. It enables:

- Dynamic decoding via descriptor
- Dynamic encoding via descriptor
- Field access by name or id
- Field mutation
- Repeated field access
- Map field access
- Enum access with name resolution
- Oneof access
- Nested message access
- Unknown field access
- Descriptor lookup
- Schema fingerprint inspection

## Example

```rust
use tpt20_core::{DecoderLimits, UnknownFieldPolicy};
use tpt20_descriptor::Descriptor;
use tpt20_reflect::DynamicMessage;

let descriptor = Descriptor::from_json(json).unwrap();
let msg = descriptor.find_message("User").unwrap();
let message = DynamicMessage::decode(msg, &descriptor, &[], &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
let name = message.get_field("name").unwrap();
let bytes = message.encode().unwrap();
```

# tpt20-json

JSON representation support for tpt20 messages (spec §14.2).

## Overview

This crate provides the shared primitives used by generated code and by the dynamic/reflection layers to convert between tpt20 values and JSON:

- [`JsonError`] — error type for all JSON conversions
- [`get_field`] — look up an object member by original name or lowerCamelCase alias
- Scalar conversion helpers implementing the spec's JSON rules: 64-bit integers are representable as strings on both encode and decode

Field-name handling, default-value emission, and unknown-field policies are applied by callers.

## Usage

```rust
use tpt20_json::{get_field, JsonError};

let obj = serde_json::json!({
    "id": 1,
    "userName": "Ada"
});

let name = get_field(&obj, "user_name").expect("lowerCamelCase lookup works");
```

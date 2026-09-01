# tpt20-stdlib

Standard library types for tpt20 (spec §15).

## Overview

This crate provides the well-known message types that ship with every `tpt20` installation. Types are defined as plain Rust structs/enums so they can be used directly, and the accompanying `.tpt` schemas (in `src/schema/`) are the canonical source of truth for code generation and descriptor exchange.

## Stability

The standard library follows the stability policy in `STABILITY.md`. Fields within these messages are part of the wire contract: adding new optional fields is safe; removing or changing existing field IDs is breaking.

## Provided types

- `Timestamp` — seconds + nanos since Unix epoch
- `Duration` — signed fixed-length span of time
- `Empty` — empty message
- `Any` — typed arbitrary value
- `Struct` / `Value` / `ListValue` — structured value representation
- `FieldMask` — field mask for partial updates
- `UUID` — 128-bit universally unique identifier
- `Decimal` — arbitrary-precision decimal
- `Money` — monetary amount with currency
- `Interval` — date/time interval
- `Pagination` — pagination cursor and limits
- `ErrorDetail` — structured error details
- Wrapper types: `BoolValue`, `BytesValue`, `DoubleValue`, `FloatValue`, `Int32Value`, `Int64Value`, `UInt32Value`, `UInt64Value`, `StringValue`

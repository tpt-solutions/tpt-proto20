# Code generation

The `tpt20-codegen-rust` crate turns a compiled schema's IR into a single,
self-contained Rust source file. This guide shows what it produces, using
the real output from the project's own codegen test fixture
(`tests/rust-codegen-tests/src/schema.tpt`).

## Generating code

From the CLI:

```sh
tpt20 gen rust --in schema/user.v1.tpt --out src/generated [--builders]
```

This writes `src/generated/<package>.rs`, e.g. `user.v1` → `user_v1.rs`
(`output_file_stem` turns `.` into `_`). Programmatically:

```rust
let compiled = tpt20_compiler::compile(&schema_src, Some("user.v1.tpt"))?;
let opts = tpt20_codegen_rust::CodegenOptions {
    builders: true,
    ..Default::default()
};
let module: String = tpt20_codegen_rust::generate_module(&compiled.ir, &opts);
```

`CodegenOptions` has three fields: `builders` (emit builder types, default
`false`), and `core_crate`/`json_crate` (the crate names generated code
imports `tpt20-core`/`tpt20-json` under — override these if you re-export or
rename those crates in your workspace).

The output is a plain `.rs` file, not a proc-macro — include it with `mod
generated;` (pointing at the file) or `include!(...)`, or wire it into a
`build.rs` the way the codegen test fixture does. It has two required
runtime dependencies: `tpt20-core` and `tpt20-json`.

## What gets generated, per message

For:

```tpt
message Address {
  1: street string;
  2: city string?;
}
```

### An owned struct

```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Address {
    pub street: String,
    pub city: Option<String>,
    #[doc(hidden)]
    pub unknown_fields: tpt20_core::RawMessage,
}
```

Every generated message struct carries an `unknown_fields` field so
[unknown fields survive a decode → mutate → re-encode round trip](wire-format.md#unknown-fields)
even through code that only knows about the fields declared at generation
time.

### Encode / decode

```rust
impl Address {
    pub fn to_raw(&self) -> tpt20_core::RawMessage;                 // schema-typed -> raw field model
    pub fn encode(&self) -> Vec<u8>;                                 // native format, default limits
    pub fn encode_canonical(&self) -> Vec<u8>;                       // deterministic form, see below
    pub fn decode(bytes: &[u8]) -> Result<Self, tpt20_core::DecodeError>;
    pub fn decode_with_limits(bytes: &[u8], limits: &tpt20_core::DecoderLimits) -> Result<Self, tpt20_core::DecodeError>;
    pub fn decode_borrowed(bytes: &[u8]) -> Result<AddressView<'_>, tpt20_core::DecodeError>;
}
```

`decode` enforces [`DecoderLimits::default()`](security-limits.md#decoder-limits);
use `decode_with_limits` at a trust boundary that needs tighter bounds.
Nested-message decoding is depth-checked recursively (`decode_inner(bytes,
limits, depth)`), so `max_depth` is enforced across the whole message tree,
not just at the top level.

`encode_canonical` reduces oneofs to their last-set member, sorts map
entries by key, and emits fields in canonical order before encoding — see
[Wire format § Canonical encoding](wire-format.md#canonical-deterministic-encoding).

### Borrowed views

```rust
pub struct AddressView<'a> {
    pub street: &'a str,
    pub city: Option<&'a str>,
    // ...
}
```

A view borrows directly from the input `&[u8]` for `string`/`bytes` fields
and nested message views, avoiding allocation on the decode path — useful
for proxies, short-lived request handling, or any place a full owned copy
would be wasted work. `Address::decode_borrowed` is the entry point;
`AddressView` also exposes its own `decode_with_limits`.

### JSON

```rust
impl Address {
    pub fn to_json_value(&self) -> Result<tpt20_json::Value, tpt20_json::JsonError>;
    pub fn to_json(&self) -> Result<String, tpt20_json::JsonError>;
    pub fn from_json_value(v: &tpt20_json::Value) -> Result<Self, tpt20_json::JsonError>;
    pub fn from_json(json: &str) -> Result<Self, tpt20_json::JsonError>;
}
```

`from_json`/`from_json_value` accept both the field's original name and its
lowerCamelCase form on decode (spec §14.2); `to_json` currently always emits
original field names (configurable name-style-on-encode is not implemented
yet — see `todo.md` Phase 8). 64-bit integers, byte fields (base64), and
enum-by-name-or-number all follow the JSON rules in spec §14.2.

### Builders (`--builders`)

```rust
impl Address {
    pub fn builder() -> AddressBuilder;
}

#[derive(Debug, Clone, Default)]
pub struct AddressBuilder { /* private fields */ }

impl AddressBuilder {
    pub fn street(mut self, v: impl Into<String>) -> Self;
    pub fn city(mut self, v: impl Into<String>) -> Self;
    pub fn build(self) -> Result<Address, BuildError>;
}
```

`build()` validates the annotations the field carries and returns
`BuildError` on violation. Implemented today: `@max_len`, `@min_len`, and
`@range` (see [Schema language § Annotations](schema-language.md#annotations)).
**Not yet implemented**: presence-requirement validation, oneof-constraint
validation, enum-validity validation, and map-key-validity validation — a
builder will currently let you build a message that violates those even
though the annotation/type system implies it shouldn't.

## Enums

```tpt
message Outer {
  enum Status { ACTIVE = 0; INACTIVE = 1; SUSPENDED = 2; }
}
```

generates:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outer_Status { ACTIVE = 0, INACTIVE = 1, SUSPENDED = 2 }

impl Outer_Status {
    pub fn from_i32(v: i32) -> Result<Self, tpt20_core::DecodeError>; // closed: rejects unknown values
    pub fn to_i32(self) -> i32;
    pub fn json_name(v: &Self) -> tpt20_json::Value;
    pub fn from_json(v: &tpt20_json::Value) -> Result<Self, tpt20_json::JsonError>; // accepts name or number
}
impl Default for Outer_Status { /* first declared value */ }
```

A nested `enum` is named `<Message>_<Enum>` in the generated module (there is
no Rust module nesting per schema message). `open enum` generates the same
shape but `from_i32` is expected to preserve rather than reject unknown
values per the enum's openness — see
[Schema language § Enums](schema-language.md#enums).

## Oneofs

```tpt
oneof contact {
  10: email_addr string;
  11: phone string;
  12: addr Address;
}
```

generates a plain Rust enum, one variant per member, named after the field's
declared type:

```rust
pub enum OuterContact {
    EmailAddr(String),
    Phone(String),
    Addr(Address),
}
```

matching spec §12.5's mutually-exclusive representation.

## Services: not yet generated

Spec §12.6 calls for generated server traits, client stubs, and streaming
interfaces per `service` block (e.g. `#[async_trait] trait UserService`).
**This is not implemented** — `tpt20-codegen-rust` only generates message
and enum code today. To build an RPC service today, hand-write the trait
against [`tpt20-rpc`'s types](rpc-model.md) (`RpcContext`,
`ServerStreamSink<T>`, etc.) using the generated message types as your
request/response payloads.

## What's generated vs. what's planned

| Spec §12 item | Status |
|---|---|
| Owned message structs | ✅ |
| `encode`/`decode`/`decode_borrowed`/`to_json`/`from_json` | ✅ |
| Borrowed view types | ✅ |
| Bytes-backed message variants | ❌ not implemented |
| Builders (opt-in) | ✅ generation; ⚠️ partial validation (see above) |
| Enums with integer conversion, open/closed semantics | ✅ |
| Oneofs as Rust enums | ✅ |
| Service server traits / client stubs / streaming interfaces | ❌ not implemented |

## Dynamic alternative: no codegen at all

If you don't want to generate code — e.g. for a generic proxy, gateway, or
admin tool that only has a descriptor at runtime — `DynamicMessage` in
`tpt20-core`/`tpt20-reflect` decodes and manipulates messages purely from
their descriptor, with the same wire format and limits. See the reflection
examples referenced from [Security limits](security-limits.md) and spec §13.

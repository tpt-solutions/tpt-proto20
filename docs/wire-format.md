# Wire format specification

This documents the native `tpt-proto20` binary wire format as implemented in
`runtime/tpt20-core` (spec §9). It is a compact, self-describing,
tag-length-value format designed to be safe to decode from untrusted input.

## Wire classes

Every field on the wire is prefixed by a tag that identifies both the field
and how to interpret the bytes that follow it:

| Wire class | Value | Meaning |
|---:|---:|---|
| `VARINT`  | 0 | Variable-length integer |
| `FIXED32` | 1 | 32-bit fixed-width, little-endian |
| `FIXED64` | 2 | 64-bit fixed-width, little-endian |
| `LEN`     | 3 | Length-delimited: a varint length, then that many bytes |

`LEN` carries strings, `bytes`, nested messages, packed repeated scalars, and
map entries — the payload's interpretation depends on the field's schema
type, not on anything in the wire bytes themselves.

## Tag encoding

```text
tag = (field_id << 3) | wire_class
```

The tag itself is encoded as a varint (`Tag::to_u64` / `Tag::from_u64` in
`wire.rs`). A tag whose low 3 bits don't map to one of the four wire classes
above is rejected as a decode error.

## Varints

Varints encode an unsigned 64-bit integer as a sequence of 7-bit groups, low
group first, with the high bit of each byte set except on the last byte
(`varint.rs::encode_varint` / `decode_varint`).

Signed integer types use **zigzag encoding** (`encode_zigzag` /
`decode_zigzag`) so small-magnitude negative numbers stay compact:

```text
zigzag(n) = (n << 1) ^ (n >> 63)
```

The decoder rejects:

- a **truncated** varint (input ends mid-sequence) → `DecodeError::Truncated`
- an **overlong** varint (more than 10 continuation bytes) → `DecodeError::VarintOverflow`
- a varint whose decoded value **overflows** 64 bits → `DecodeError::VarintOverflow`

## Scalar types

| Schema type | Wire class | Encoding |
|---|---|---|
| `bool`, `int32`, `int64`, `uint32`, `uint64` | VARINT | plain varint |
| `sint32`, `sint64` | VARINT | zigzag varint |
| `fixed32`, `sfixed32`, `float32` | FIXED32 | 4-byte little-endian |
| `fixed64`, `sfixed64`, `float64` | FIXED64 | 8-byte little-endian |
| `string`, `bytes` | LEN | varint length + raw bytes (UTF-8 validated for `string`) |

## Length-delimited fields

```text
length: varint
payload: length bytes
```

The decoder rejects a length that would overflow `usize`, a length whose
payload runs past the end of the input (`DecodeError::Truncated`), invalid
UTF-8 in a `string` field (`DecodeError::InvalidUtf8`), and any payload
longer than the applicable `DecoderLimits` field (`DecodeError::LimitExceeded`
— see [Security limits](security-limits.md)).

## Repeated fields

Repeated scalar fields may be encoded **packed** (all values concatenated
inside a single `LEN` field) or **unpacked** (one tag+value per element).
Generated code always *encodes* packed form for scalar repeated fields
(`encode_packed_varints` / `_fixed32` / `_fixed64`) but *decoding* accepts
either form, and even a mix of the two across repeated appearances of the
same field ID in one message — this matches spec §9.6 ("decoders must accept
both packed and unpacked forms").

## Maps

A map field is encoded as a sequence of synthetic two-field entry messages,
each carrying the map's key as field 1 and its value as field 2:

```text
map_field (LEN) {
  1: key
  2: value
}
map_field (LEN) {
  1: key
  2: value
}
...
```

- Duplicate keys are accepted; the **last** occurrence wins when decoding
  into a `HashMap`/`BTreeMap`.
- Canonical mode sorts entries by key (`RawMessage::canonical_sort_map_entries`).
- Entry count is bounded by `DecoderLimits::max_map_entries`.

## Oneofs

Oneof members are encoded as ordinary fields with their own field IDs — there
is no wire-level grouping. If more than one member of the same oneof group
appears while decoding, **the last one wins**; generated code surfaces the
group as a single Rust enum so only one variant can be held at a time.

## Unknown fields

Fields with an ID the decoder doesn't recognize are handled per
`UnknownFieldPolicy`:

| Policy | Behavior |
|---|---|
| `Preserve` (default) | Kept as raw `Field`s on `RawMessage`/generated struct `unknown_fields`, and re-emitted verbatim on re-encode. |
| `Discard` | Silently dropped. |
| `Fail` | Decoding returns `DecodeError::UnknownFieldForbidden`. |

Preserving unknown fields is what lets an older reader round-trip a message
containing fields from a newer schema version without losing data.

## Canonical (deterministic) encoding

`RawMessage::encode_canonical` (and the `encode_canonical()` method on
generated types) produces a byte-for-byte deterministic encoding suitable for
hashing, signing, and content addressing:

- fields are emitted in a total, stable order
- map entries are sorted by key (`canonical_sort_map_entries`)
- oneof groups are reduced so only the last-set member is emitted
  (`canonical_reduce_oneofs`)
- varints use their unique canonical (shortest) representation — the same
  representation `encode_varint` always produces
- unknown fields are ordered deterministically alongside known fields

Two messages that are semantically equal always produce identical canonical
bytes, independent of field insertion order or which packed/unpacked form a
repeated field originally arrived in.

## Optional envelope

For schema-addressed storage or event systems, a message may optionally be
wrapped:

```tpt
message Envelope {
  1: schema_id bytes;
  2: schema_version string;
  3: payload bytes;
}
```

`tpt20_core::Envelope` implements this directly. It is never required for
RPC payloads — RPC framing (see [RPC model § Transport](rpc-model.md#transport-and-framing))
carries the schema context out of band via the method name.

## Error reference

Decode failures are reported as one of `tpt20_core::DecodeError`'s variants:
`Truncated`, `VarintOverflow`, `InvalidLength`, `InvalidUtf8`,
`LimitExceeded { limit }`, `DepthExceeded`, `FieldCountExceeded`,
`RepeatedEntriesExceeded`, `MapEntriesExceeded`, `MalformedScalar`,
`MalformedMapEntry`, `InvalidEnumValue(i32)`, `WireClassMismatch { field_id }`,
`UnknownFieldForbidden`. None of these variants carry borrowed data, so
errors can be freely stored, logged, or compared. Encoding is effectively
infallible in the current implementation (`EncodeError` has only an
`Internal` variant reserved for codec bugs).

## Implementation notes

- Decoding never uses `unsafe` (spec §18.7); all arithmetic on lengths,
  offsets, and counts is checked.
- The core codec (`tpt20-core`) has no schema awareness — it operates on
  `(field_id, wire_class, value)` triples (`RawMessage`/`Field`/`Value`).
  Schema-aware decoding into named struct fields is a layer generated code
  and `DynamicMessage` (see [Code generation](code-generation.md) and the
  reflection API) build on top of it.

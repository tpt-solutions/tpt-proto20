# Conformance vectors

Language-neutral JSON test vectors (spec §22) for `tpt-proto20`, run with

```sh
tpt20 conformance [--directory conformance/vectors] [--test <file|suite|case>]
```

They are hand-derived from the written specification (the wire format's
`tag = (field_id << 3) | wire_class` rule, the canonical-encoding order, the
text format grammar, the schema diagnostics), not generated from this
implementation, so another implementation can run the same files.

Each file is `{"suite": ..., "cases": [...]}`; a case has a `name`, a `kind`
and kind-specific fields. See the module docs of
[`tools/tpt20-cli/src/conformance.rs`](../tools/tpt20-cli/src/conformance.rs)
for the exact format:

| kind | checks |
|---|---|
| `decode` | bytes decode to the expected fields, or fail with the expected error; optional `limits`, `policy`, `known_ids` |
| `canonical` | fields encode canonically to exactly the given bytes (and re-encode idempotently) |
| `roundtrip` | decode then encode reproduces the bytes |
| `text` | text format ↔ wire bytes for a given schema and message |
| `schema` | a schema compiles, or is rejected with a given diagnostic code |

The larger Rust suite (`tools/tpt20-conformance`, `tests/conformance`) runs
with `cargo test`.

# Polyglot code generation (Go, Java, Python)

Rust is the reference implementation, but schemas can also be compiled to **Go**,
**Java** and **Python**:

```sh
tpt20 gen go     --in schema.tpt --out gen/go
tpt20 gen java   --in schema.tpt --out gen/java
tpt20 gen python --in schema.tpt --out gen/python
```

Each backend (`compiler/tpt20-codegen-backends`) is driven by the neutral IR
through a shared, resolved schema model and emits **self-contained source**: the
generated message code plus a small runtime file next to it
(`tpt20_runtime.go`, `Tpt20Runtime.java`, `tpt20_runtime.py`) — there is
nothing to install.

| Language | Output | Notes |
| --- | --- | --- |
| Go 1.21+ | `go.mod`, `<pkg>.go`, `tpt20_runtime.go` | structs, pointer = explicit presence, oneofs as interface + variant structs, generics in the runtime |
| Java 11+ | `<pkg>/*.java` | one class per message, enum (int constants) and oneof; boxed type or `null` = explicit presence; `uint32`/`fixed32` are `int` bit patterns |
| Python 3.8+ | `<pkg>.py`, `tpt20_runtime.py` | dataclasses; `None` = explicit presence; oneof holds a variant object |

## What is implemented

Binary encode and decode of the native wire format, with the same decoder
limits as the Rust runtime (`max_depth`, field counts, string/bytes/repeated/map
bounds, unknown-field budget), unknown-field preservation, packed and unpacked
repeated scalars, maps (entries encoded in key order, absent key/value take
defaults), oneofs (last one wins), open/closed enums and explicit presence.

**Not implemented in the other languages:** JSON and text formats, builders and
validation annotations, borrowed views, canonical encoding
(`encode_canonical`), and services/RPC — those remain Rust-only for now.

## How it is verified

`tests/rust-codegen-tests/tests/polyglot.rs` generates all three languages from
the same schema, compiles/runs them with the local toolchain (a language whose
toolchain is missing is skipped) and feeds each the same ~1 600 byte strings:
valid samples, merged concatenations, unknown fields of every wire class, every
truncation, and random mutations. Each implementation decodes and re-encodes;
the result must be **byte-identical to Rust's**, and an input that Rust rejects
must be rejected by all of them. The harness found and fixed two Rust bugs
(tags with a field id beyond 32 bits aliased another field; see
`DecodeError::FieldIdOutOfRange`).

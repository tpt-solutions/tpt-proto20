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

**Services (Go and Python).** With services in the schema, `gen go` / `gen
python` also emit a client and a server stub per service plus an RPC runtime
(`tpt20_rpc.go` — standard library only, **Go 1.24+**; `tpt20_rpc.py` — asyncio,
needs `pip install h2`). They speak the same HTTP/2 protocol as the Rust runtime
(see [RPC model](rpc-model.md)): all four streaming shapes, request/response
metadata (including `-bin` keys), status codes with messages, `grpc-timeout`
deadlines and client cancellation, and the server answers `UNIMPLEMENTED` for
unknown methods. `--no-services` skips them.

```go
srv := NewTpt20Server()
RegisterPinger(srv, impl{})              // impl implements PingerHandler
go srv.Serve(listener)
c := NewPingerClient(Tpt20Dial("host:50051"))
reply, err := c.Ping(ctx, md, &PingRequest{Text: "hi"})
```

```python
server = rpc.Server(); register_pinger(server, Impl())   # Impl subclasses PingerBase
await server.serve("127.0.0.1", 50051)
client = PingerClient(rpc.Channel("host", 50051))
reply = await client.ping(PingRequest(text="hi"), timeout=2.0)
```

**Java has no RPC runtime**: the JDK's HTTP client cannot read HTTP/2 trailers
(where the status travels) and has no HTTP/2 server, so a useful Java RPC layer
needs a third-party stack such as grpc-java or Netty. Use the generated
messages with it, or talk to a tpt20 server through the gRPC compatibility
adapter.

**Not implemented in the other languages:** JSON and text formats, builders and
validation annotations, borrowed views, canonical encoding
(`encode_canonical`), TLS/mTLS and QUIC, message compression, reflection and
health services, and observability hooks — those remain Rust-only for now.

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

`tests/rust-codegen-tests/tests/polyglot_rpc.rs` runs a scripted scenario (unary
with metadata, server/client/bidi streaming with real interleaving, error
status, unknown method, deadline, cancellation) in every direction that exists:
Go client ↔ Rust server, Rust client ↔ Go server, Python client ↔ Rust server,
Rust client ↔ Python server, and Go ↔ Python with no Rust involved. The same
scenario also runs through a Go `ReverseProxy` and a round-robin load balancer
over two Rust backends (trailers and streaming survive the hop). It found a
protocol bug on the first run: client- and bidi-streaming calls opened with an
empty placeholder frame that servers dropped, so a standard client's first
message was lost — the Rust client now sends no placeholder and the server
treats the first frame as a real message. CI installs Go, Java, Python and
`h2` so these run on every push.

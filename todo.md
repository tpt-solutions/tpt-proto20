# tpt-proto20 — Project Todo

**Owner:** TPT Solutions
**License:** MIT OR Apache-2.0 (dual-licensed)
**Implementation model:** Clean-room (see Phase 0 provenance/AI policy)
**Primary language:** Rust

Source of truth for this checklist: `spec.txt` (30-section design document).

**Explicitly out of scope for core** (per spec §4) — not tracked below:
message brokers, databases, object-relational mapping, UI state synchronization,
general-purpose application frameworks. These may be built *on top of* tpt-proto20 later,
but are not part of this project.

Phases are ordered by build dependency (compiler track → runtime → compat adapters →
tooling → conformance/perf → docs/release → polyglot stretch), matching the architecture
diagram in spec §5 and the repo layout in spec §26.

---

## Phase 0 — Project Foundation & Governance

- [x] `git init` the repository
- [x] `.gitignore` for Rust (`target/`, etc.)
- [x] Root Cargo workspace `Cargo.toml`
- [ ] Create workspace layout per spec §26:
  - [x] `compiler/tpt20-language/`
  - [x] `compiler/tpt20-ir/`
  - [x] `compiler/tpt20-descriptor/`
  - [x] `compiler/tpt20-compiler/`
  - [x] `compiler/tpt20-codegen-rust/`
  - [ ] `compiler/tpt20-codegen-backends/`
  - [x] `runtime/tpt20-core/`
  - [ ] `runtime/tpt20-reflect/`
  - [x] `runtime/tpt20-json/`
  - [ ] `runtime/tpt20-text/`
  - [ ] `runtime/tpt20-stdlib/`
  - [ ] `runtime/tpt20-rpc/`
  - [ ] `runtime/tpt20-transport/`
  - [x] `runtime/tpt20-observability/`
  - [ ] `compat/tpt20-compat-protobuf/`
  - [ ] `compat/tpt20-compat-grpc/`
  - [x] `tools/tpt20-cli/`
  - [ ] `tools/tpt20-lint/`
  - [ ] `tools/tpt20-diff/`
  - [ ] `tools/tpt20-conformance/`
  - [ ] `tools/tpt20-registry/`
  - [ ] `docs/`
  - [ ] `examples/`
  - [x] `tests/`
  - [ ] `fuzz/`
  - [ ] `benches/`
  - [x] `provenance/`
- [x] `LICENSE-MIT` (TPT Solutions)
- [x] `LICENSE-APACHE` (TPT Solutions)
- [x] `COPYRIGHT` file (TPT Solutions)
- [x] Set `license = "MIT OR Apache-2.0"` and `authors`/`TPT Solutions` in workspace `Cargo.toml`
- [x] `CONTRIBUTING.md`
  - [x] Document clean-room policy (spec §25.1): allowed inputs (public specs, original
        design notes/tests, independently created test vectors) vs. disallowed inputs
        (upstream implementation code, proprietary implementations, copied codegen
        templates, AI prompts containing upstream source)
  - [x] Document AI-assisted contribution policy (spec §25.2): human review, testing,
        documentation, similarity checks, provenance recording
- [x] `provenance/` directory with initial policy docs
- [x] Baseline CI pipeline (build, test, `fmt --check`, `clippy`)
- [x] `README.md` skeleton (project summary, vision, links to docs/)

---

## Phase 1 — Schema Language (`tpt20-language`, spec §6)

- [x] Define `.tpt` file extension and top-level file grammar
- [x] Lexer/tokenizer for `.tpt` source
- [x] Parser producing an AST
- [x] `package` declarations (e.g. `package user.v1;`)
- [x] `import` statements
- [x] Message declarations with numeric field IDs (`1: id int64;`)
- [x] Enforce field IDs are part of the wire contract (no silent reuse post-publish)
- [x] Implicit presence semantics (absence == default)
- [x] Explicit presence syntax `?` and semantics (absence distinguishable from default)
- [x] Message-field presence-awareness
- [x] Confirm "required" fields are NOT part of the language (reject if attempted)
- [x] Repeated field syntax (`repeated`) and list semantics
- [x] Map syntax `map<K, V>`
  - [x] Restrict key types to scalar or string types
  - [x] Validate value types
  - [ ] Deterministic encoding ordering rule (spec'd, enforced later in Phase 4)
  - [ ] Runtime size limit hook (enforced later in Phase 4)
- [x] Oneof syntax (`oneof name { ... }`) and mutual-exclusivity semantics
- [x] Enum declarations with stable numeric values
  - [x] Enum value aliases with explicit annotation
  - [x] Open vs. closed enum annotation/semantics
- [x] Service declarations
  - [x] Unary method syntax
  - [x] Server-streaming method syntax (`stream` on return type)
  - [x] Client-streaming method syntax (`stream` on request type)
  - [x] Bidirectional-streaming method syntax
- [x] Annotation syntax (`@name(args)`)
  - [x] Core/standardized annotations (`@max_len`, `@range`, `@pattern`, etc.)
  - [x] Custom annotation registry mechanism
- [x] Reserved field ID / reserved name syntax
- [x] AST data structures for all of the above
- [x] Parser test suite, including full round-trip of the spec §6.1 example schema
- [x] Error recovery / helpful parse error messages (feeds into Phase 2 diagnostics)

---

## Phase 2 — Semantic Analysis & Compiler Core (`tpt20-compiler`, spec §7, §20)

- [x] Wire up full compiler pipeline: lexer → parser → AST → semantic analysis →
      compatibility checks → IR generation → descriptor generation → code generation
- [x] Semantic analysis pass
  - [x] Duplicate field ID detection within a message
  - [x] Duplicate enum value ID detection (respecting alias annotation)
  - [x] Duplicate message/enum/service names detection
  - [x] Unresolved import detection
  - [x] Oneof field validity rules
  - [x] Map key/value type validity rules
  - [x] Annotation argument validity checks
- [x] Compatibility-change detector
  - [x] Detect safe additions (optional field, repeated field, new enum value in open
        enum, new service method, reserved removed field IDs)
  - [x] Detect warning changes (rename field keeping ID, rename method, doc/annotation
        semantic changes)
  - [x] Detect breaking changes (field type change, field ID change, field removal
        without reservation, method removal without policy, streaming direction change,
        incompatible request/response type change)
  - [x] Classify every detected change as SAFE / WARNING / BREAKING
- [x] Diagnostics engine
  - [x] File path in diagnostic
  - [x] Line number
  - [x] Column number
  - [x] Span
  - [x] Severity
  - [x] Error code (e.g. `E0042`)
  - [x] Human-readable explanation
  - [x] Suggested fix where possible
  - [x] Match rendered format to spec §7.3 example output
- [x] Schema fingerprint
  - [x] Derive stable fingerprint from canonical descriptor
  - [x] Verify fingerprint stability across semantically-identical re-serializations
  - [x] Wire fingerprint into registry / dynamic validation / debugging / migration use
        cases
- [x] Schema history manifest support (spec §20.5)
  - [x] Record schema versions
  - [x] Record fingerprints
  - [x] Record compatibility policies
  - [x] Record migration notes
  - [x] Record reserved IDs
  - [x] Record deprecation dates

---

## Phase 3 — IR & Descriptor Model (`tpt20-ir`, `tpt20-descriptor`, spec §8)

- [x] Define neutral IR types
  - [x] Packages
  - [x] Files
  - [x] Messages
  - [x] Fields
  - [x] Enums
  - [x] Enum values
  - [x] Oneofs
  - [x] Maps
  - [x] Services
  - [x] Methods
  - [x] Annotations
  - [x] Source locations
  - [x] Compatibility metadata
  - [x] Schema fingerprint
- [x] Descriptor model (runtime representation of compiled schema)
  - [x] Binary serialization of descriptors
  - [x] JSON serialization of descriptors
  - [x] Dynamic lookup support (by name/ID)
  - [x] Reflection support hooks (consumed by Phase 7)
  - [x] Cross-language interchange format
  - [x] Schema registry storage format

---

## Phase 4 — Native Binary Wire Format & Core Runtime (`tpt20-core`, spec §9, §18)

- [x] Wire classes: VARINT (0), FIXED32 (1), FIXED64 (2), LEN (3)
- [x] Tag encoding: `tag = (field_id << 3) | wire_class`, encoded as varint
- [x] Scalar type support: `bool`, `int32`, `int64`, `uint32`, `uint64`, `sint32`, `sint64`,
      `fixed32`, `fixed64`, `sfixed32`, `sfixed64`, `float32`, `float64`, `string`, `bytes`
- [x] Varint encoding/decoding (7-bit groups)
  - [x] Zigzag encoding for signed varint types
  - [x] Reject truncated varints
  - [x] Reject overlong varints
  - [x] Reject overflowing varints
  - [x] Reject malformed payloads
- [x] Length-delimited field encoding (varint length + payload bytes)
  - [x] Reject negative lengths
  - [x] Reject overflowing lengths
  - [x] Reject truncated payloads
  - [x] Reject invalid UTF-8 in string fields
  - [x] Reject payloads exceeding configured limits
- [x] Repeated field encoding/decoding
  - [x] Packed encoding
  - [x] Unpacked encoding
  - [x] Accept mixed packed/unpacked input for compatible scalar repeated fields
- [x] Map encoding/decoding
  - [x] Encode maps as repeated synthetic map-entry messages (key = field 1, value = field 2)
  - [x] Accept duplicate map entries on decode
  - [x] Later value overrides earlier value by default
  - [x] Deterministic map-entry ordering in canonical mode
  - [x] Enforce decoder limits on map entries
- [x] Oneof encoding/decoding
  - [x] Encode oneof fields as normal fields
  - [x] Last field wins when multiple oneof members appear on decode
  - [x] Expose oneof as mutually-exclusive generated type (ties into Phase 5)
- [x] Unknown field handling
  - [x] Preserve policy (default)
  - [x] Discard policy
  - [x] Fail policy
  - [x] Ensure preserved unknown fields are re-encodable
- [x] Canonical deterministic encoding mode
  - [x] Canonical field order
  - [x] Canonical map ordering
  - [x] Canonical unknown-field ordering
  - [x] Canonical varint representation
  - [x] Canonical repeated-field ordering where applicable
  - [x] Canonical oneof serialization behavior
  - [x] Validate suitability for hashing/signatures/auditing/reproducible builds/content
        addressing
- [x] Optional `Envelope` message (`schema_id: bytes`, `schema_version: string`,
      `payload: bytes`) — optional, not required for normal RPC payloads
- [x] `DecoderLimits` struct
  - [x] `max_message_bytes`
  - [x] `max_depth`
  - [x] `max_field_count`
  - [x] `max_unknown_field_bytes`
  - [x] `max_string_bytes`
  - [x] `max_bytes_field_bytes`
  - [x] `max_repeated_entries`
  - [x] `max_map_entries`
  - [x] Enforce every limit in the decode path with sane defaults
- [x] Checked-arithmetic integer safety throughout encode/decode
- [x] UTF-8 validation for all string fields
- [x] Recursion/depth bounding for nested messages
- [ ] Allocation bounding/predictability
- [x] Enforce "no unsafe in core decoding paths" default policy
  - [x] Any exception is isolated, documented, tested, feature-gated, and justified by
        measurable benefit

---

## Phase 5 — Rust Code Generation (`tpt20-codegen-rust`, spec §12)

- [x] Generate owned message structs from message schemas
- [x] Generated methods per message:
  - [x] `encode(&self) -> Vec<u8>`
  - [x] `decode(bytes: &[u8]) -> Result<Self, DecodeError>`
  - [x] `decode_borrowed(bytes: &[u8]) -> Result<XView<'_>, DecodeError>`
  - [x] `to_json(&self) -> Result<String, JsonError>`
  - [x] `from_json(json: &str) -> Result<Self, JsonError>`
- [x] Generate borrowed view types (e.g. `UserView<'a>`) mirroring owned structs
- [ ] Generate bytes-backed message variants
- [x] Generate builders (opt-in)
  - [ ] Presence-requirement validation
  - [ ] Oneof-constraint validation
  - [x] Annotation-constraint validation (`@max_len`, `@min_len`, `@range`)
  - [ ] Enum-validity validation
  - [ ] Map-key-validity validation
- [x] Generate Rust enums for schema enums with integer conversion support
  - [x] Respect open/closed unknown-value semantics
- [x] Generate Rust enums for oneofs (e.g. `enum ContactMethod { Email(String), ... }`)
- [ ] Generate service code
  - [ ] Server traits (`#[async_trait]`)
  - [ ] Client stubs
  - [ ] Streaming interfaces (server/client/bidi)
  - [ ] Metadata helpers
  - [ ] Deadline helpers
  - [ ] Cancellation helpers
- [x] Wire `tpt20 gen rust --in schema --out src/generated` CLI command (stub now, full CLI
      in Phase 16)

---

## Phase 6 — Runtime Message Model: Dynamic & Bytes-backed (`tpt20-core`, spec §11.3–11.4)

- [ ] `DynamicMessage::decode(descriptor, bytes)` — descriptor-driven decode with no
      compile-time generated types
- [ ] Dynamic field lookup by name/ID
- [ ] Dynamic field mutation
- [ ] Dynamic unknown-field access
- [ ] Dynamic JSON conversion
- [ ] Dynamic text conversion
- [ ] Bytes-backed message slicing/sharing utilities
  - [ ] Validate against proxy use case
  - [ ] Validate against cache use case
  - [ ] Validate against streaming-pipeline use case
  - [ ] Validate against zero-copy-gateway use case

---

## Phase 7 — Reflection (`tpt20-reflect`, spec §13)

- [ ] Dynamic decoding via descriptor
- [ ] Dynamic encoding via descriptor
- [ ] Field access API
- [ ] Field mutation API
- [ ] Repeated field access API
- [ ] Map access API
- [ ] Enum access API
- [ ] Oneof access API
- [ ] Nested message access API
- [ ] Unknown field access API
- [ ] Descriptor lookup API
- [ ] Schema fingerprint inspection API
- [ ] Example: `message.get_field("name")`, `message.encode()` matches spec §13 example
- [ ] Validate reflection enables: proxies, gateways, debuggers, admin tools, schema
      registries, dynamic routing, test harnesses

---

## Phase 8 — JSON & Text Representations (`tpt20-json`, `tpt20-text`, spec §14)

- [x] JSON mapping
  - [x] Support original field names on decode
  - [x] Support lowerCamelCase field names on decode
  - [ ] Configurable field-name style on encode (always emits original names)
  - [x] 64-bit integers representable as JSON strings
  - [x] Bytes fields as base64
  - [x] Enums representable by name or by number
  - [ ] Configurable default-value emission (defaults always omitted)
  - [ ] Configurable unknown-field handling
- [ ] Text format
  - [ ] Printer (message → human-readable text, matching spec §14.3 example)
  - [ ] Parser (text → message)
  - [ ] Repeated field support
  - [ ] Map field support
  - [ ] Oneof support
  - [ ] Nested message support
  - [ ] Deterministic output ordering

---

## Phase 9 — Standard Library Types (`tpt20-stdlib`, spec §15)

- [x] `Timestamp`
- [x] `Duration`
- [x] `Empty`
- [x] `Any`
- [x] `Struct`
- [x] `Value`
- [x] `ListValue`
- [x] `FieldMask`
- [x] `UUID`
- [x] `Decimal`
- [x] `Money`
- [x] `Interval`
- [x] `Pagination`
- [x] `ErrorDetail`
- [x] Wrapper types: `BoolValue`, `BytesValue`, `DoubleValue`, `FloatValue`, `Int32Value`,
      `Int64Value`, `UInt32Value`, `UInt64Value`, `StringValue`
- [x] Define stability/versioning policy for the standard library

---

## Phase 10 — RPC System (`tpt20-rpc`, spec §16)

- [ ] `RpcContext` struct
  - [ ] `deadline: Deadline`
  - [ ] `cancellation: CancellationToken`
  - [ ] `metadata: Metadata`
  - [ ] `trace: TraceContext`
  - [ ] `peer: Option<PeerInfo>`
  - [ ] `extensions: Extensions`
  - [ ] `ctx.is_expired()`
  - [ ] `ctx.remaining_time()`
  - [ ] `ctx.metadata()`
  - [ ] `ctx.trace()`
- [ ] Unary call support
- [ ] Server-streaming support (`ServerStreamSink<T>`), backpressure-aware
- [ ] Client-streaming support (`ClientStreamSource<T>`), backpressure-aware
- [ ] Bidirectional-streaming support (`BidiStream<T>`), backpressure-aware
- [ ] Status codes: `OK`, `CANCELLED`, `UNKNOWN`, `INVALID_ARGUMENT`, `DEADLINE_EXCEEDED`,
      `NOT_FOUND`, `ALREADY_EXISTS`, `PERMISSION_DENIED`, `RESOURCE_EXHAUSTED`,
      `FAILED_PRECONDITION`, `ABORTED`, `OUT_OF_RANGE`, `UNIMPLEMENTED`, `INTERNAL`,
      `UNAVAILABLE`, `DATA_LOSS`, `UNAUTHENTICATED`
- [ ] Rich error details
  - [ ] `RpcError::invalid_argument(...).with_details(...)` style API
  - [ ] Compatibility with descriptor-based dynamic decoding of error details
- [ ] Metadata handling
  - [ ] Lowercase metadata keys
  - [ ] Binary metadata standard suffix convention
  - [ ] Metadata size limit enforcement
  - [ ] Reserved metadata key protection
- [ ] Compression support
- [ ] Authentication hooks
- [ ] Authorization hooks
- [ ] Retry support
- [ ] Backpressure support end-to-end

---

## Phase 11 — Transport Layer (`tpt20-transport`, spec §17)

- [ ] HTTP/2 transport (required production transport)
  - [ ] Multiplexed streams
  - [ ] Trailers
  - [ ] Flow control
  - [ ] Stream reset handling
  - [ ] GOAWAY handling
  - [ ] Keepalive/ping behavior
  - [ ] TLS with ALPN
  - [ ] Cleartext h2c for local development (explicit opt-in only)
- [ ] Message framing: 1-byte flags + 4-byte big-endian length + N-byte payload
  - [ ] Compression-enabled flag
  - [ ] Reserved bits for future protocol extensions
- [ ] In-process transport
  - [ ] Usable in tests
  - [ ] Usable in embedded systems
  - [ ] Usable in local development
  - [ ] Usable in benchmarking
  - [ ] Usable in fuzzing
- [ ] Optional QUIC/HTTP3 transport
- [ ] Optional custom stream transport extension point

---

## Phase 12 — Observability (`tpt20-observability`, spec §19)

- [x] Metrics
  - [x] Requests started
  - [x] Requests completed
  - [x] Request duration
  - [x] Active streams
  - [x] Cancelled requests
  - [x] Deadline-exceeded requests
  - [x] Bytes sent
  - [x] Bytes received
  - [x] Messages sent
  - [x] Messages received
  - [x] Decode failures
  - [x] Encode failures
  - [x] Connection errors
  - [x] Stream resets
  - [x] Labels: `service`, `method`, `status`, `streaming_type`, `transport`
- [x] Tracing integration
  - [x] Span attribute `rpc.system`
  - [x] Span attribute `rpc.service`
  - [x] Span attribute `rpc.method`
  - [x] Span attribute `rpc.status`
  - [x] Span attribute `rpc.schema_fingerprint`
- [x] Structured logging
  - [x] Request ID
  - [x] Service
  - [x] Method
  - [x] Status
  - [x] Deadline
  - [x] Cancellation reason
  - [x] Peer info where allowed
  - [x] Schema fingerprint where useful
- [ ] CLI schema-aware debugging support (`tpt20 decode --schema ... --message ...`),
      full CLI wiring in Phase 16

---

## Phase 13 — RPC Security Hardening (`tpt20-rpc` / `tpt20-transport`, spec §18.6)

- [ ] TLS support
- [ ] mTLS support
- [ ] Token authentication
- [ ] Metadata-based authentication
- [ ] Authorization middleware
- [ ] Peer inspection
- [ ] Rate-limiting hooks
- [ ] Request limits
- [ ] Deadline enforcement at the transport/RPC boundary

---

## Phase 14 — Compatibility Adapter: Protobuf (`tpt20-compat-protobuf`, spec §10.1–10.2)

- [ ] `.proto` schema import
  - [ ] proto2 support
  - [ ] proto3 support
  - [ ] Editions support where feasible
  - [ ] Messages
  - [ ] Enums
  - [ ] Oneofs
  - [ ] Maps
  - [ ] Services
  - [ ] Options where meaningful
  - [ ] Reserved fields
  - [ ] Extensions where feasible
  - [ ] `tpt20 import-proto user.proto --out user.tpt` (CLI wiring in Phase 16)
- [ ] Protobuf wire adapter
  - [ ] `decode_protobuf(bytes)` conceptual API
  - [ ] `encode_protobuf()` conceptual API
  - [ ] Round-trip fidelity testing against real protobuf messages
- [ ] Golden-vector / differential testing against an established protobuf implementation

---

## Phase 15 — Compatibility Adapter: gRPC (`tpt20-compat-grpc`, spec §10.3)

- [ ] HTTP/2 framing compatible with gRPC
- [ ] Protobuf-compatible message payload support
- [ ] Status code mapping (tpt20 ↔ gRPC)
- [ ] Metadata mapping (tpt20 ↔ gRPC)
- [ ] Deadline mapping (tpt20 ↔ gRPC)
- [ ] Streaming mode mapping (unary/server/client/bidi)
- [ ] Health-checking protocol support
- [ ] gRPC reflection support where feasible

---

## Phase 16 — Developer Tooling / CLI
(`tpt20-cli`, `tpt20-lint`, `tpt20-diff`, `tpt20-registry`, spec §21)

- [ ] CLI command: `tpt20 init`
- [ ] CLI command: `tpt20 check`
- [ ] CLI command: `tpt20 fmt` (schema formatter)
- [ ] CLI command: `tpt20 lint` (with configurable rule set)
- [ ] CLI command: `tpt20 diff` — SAFE/WARNING/BREAKING output matching spec §21.4 format
- [x] CLI command: `tpt20 gen rust` (wires to Phase 5 Rust codegen; `gen` for other
      backends deferred to Phase 21)
- [ ] CLI command: `tpt20 descriptors`
- [ ] CLI command: `tpt20 decode`
- [ ] CLI command: `tpt20 encode`
- [ ] CLI command: `tpt20 json-to-binary`
- [ ] CLI command: `tpt20 binary-to-json`
- [ ] CLI command: `tpt20 text-to-binary`
- [ ] CLI command: `tpt20 binary-to-text`
- [ ] CLI command: `tpt20 import-proto`
- [ ] CLI command: `tpt20 conformance`
- [ ] CLI command: `tpt20 call` (RPC debugger)
  - [ ] JSON input support
  - [ ] Binary input support
  - [ ] Metadata support
  - [ ] Deadline support
  - [ ] TLS configuration support
  - [ ] Compression configuration support
  - [ ] Streaming call support
- [ ] CLI command: `tpt20 health`
- [ ] CLI command: `tpt20 reflect`
- [ ] CLI command: `tpt20 registry publish`
- [ ] `tpt20-registry` service/storage design
  - [ ] Schema storage keyed by fingerprint/version
  - [ ] Publish workflow
  - [ ] Lookup/fetch workflow

---

## Phase 17 — Conformance & Testing (`tpt20-conformance`, spec §22)

- [ ] Native conformance suite
  - [ ] Schema parsing conformance
  - [ ] Semantic analysis conformance
  - [ ] Wire encoding conformance
  - [ ] Wire decoding conformance
  - [ ] Canonical encoding conformance
  - [ ] JSON mapping conformance
  - [ ] Text mapping conformance
  - [ ] Reflection conformance
  - [ ] Dynamic message conformance
  - [ ] RPC behavior conformance
  - [ ] Streaming behavior conformance
  - [ ] Deadline behavior conformance
  - [ ] Cancellation behavior conformance
  - [ ] Security limit conformance
- [ ] Compatibility conformance suite
  - [ ] Protobuf schema import conformance
  - [ ] Protobuf binary decoding conformance
  - [ ] Protobuf binary encoding conformance
  - [ ] gRPC-compatible RPC behavior conformance
  - [ ] Status mapping conformance
  - [ ] Metadata mapping conformance
  - [ ] Streaming semantics conformance
- [ ] Fuzz targets
  - [ ] Binary decoder fuzz target
  - [ ] JSON decoder fuzz target
  - [ ] Text parser fuzz target
  - [ ] Schema parser fuzz target
  - [ ] Descriptor decoder fuzz target
  - [ ] Dynamic message decoder fuzz target
  - [ ] RPC framing fuzz target
  - [ ] Metadata parsing fuzz target
- [ ] Property-based roundtrip testing
  - [ ] `encode -> decode -> equal`
  - [ ] `decode -> encode -> decode -> equal`
- [ ] Rust ↔ Rust interoperability test baseline
      (non-Rust interop tracked in Phase 21 stretch goals)

---

## Phase 18 — Performance & Benchmarking (`benches/`, spec §23)

- [ ] Benchmark: small messages
- [ ] Benchmark: large messages
- [ ] Benchmark: nested messages
- [ ] Benchmark: repeated fields
- [ ] Benchmark: packed fields
- [ ] Benchmark: maps
- [ ] Benchmark: unknown fields
- [ ] Benchmark: dynamic decoding
- [ ] Benchmark: borrowed decoding
- [ ] Benchmark: JSON conversion
- [ ] Benchmark: unary RPC
- [ ] Benchmark: streaming RPC
- [ ] Benchmark: concurrent streams
- [ ] Benchmark: cancellation storms
- [ ] Benchmark: deadline storms
- [ ] Benchmark: TLS overhead
- [ ] Benchmark: compression overhead
- [ ] Profiling pass and optimization backlog based on results
- [ ] Confirm performance goals from spec §23 (fast varints, minimal allocations, efficient
      repeated/map handling, efficient streaming, monomorphized codegen, optional
      zero-copy decode, bounded memory, low-overhead observability)

---

## Phase 19 — Documentation (`docs/`, spec §27.9)

- [ ] Quickstart guide
- [ ] Schema language reference
- [ ] Wire format specification document
- [ ] RPC model documentation
- [ ] Compatibility adapter guides (protobuf + gRPC)
- [ ] Security limits documentation
- [ ] Observability guide
- [ ] Code generation guide
- [ ] CLI usage reference
- [ ] Provenance policy document

---

## Phase 20 — Versioning, Release & Governance (spec §24, §27, §28.4)

- [ ] Adopt semantic versioning for the project overall
- [ ] Document stability policy for public APIs
- [ ] Document stability policy for generated code
- [ ] Document stability policy for the wire format
- [ ] Document stability policy for the descriptor format
- [ ] Document stability policy for CLI output
- [ ] Document stability policy for registry APIs
- [ ] Schema package versioning convention (e.g. `package user.v1;`)
- [ ] Compatibility policy: wire format changes backward-compatible or protocol-version
      gated
- [ ] Compatibility policy: descriptor format versioning
- [ ] Compatibility policy: generated code stability documentation
- [ ] Compatibility policy: CLI breaking changes follow semver
- [ ] Community governance documentation
- [ ] v1.0 acceptance-criteria sign-off checklist (mirrors spec §27.1–27.9):
  - [ ] §27.1 Schema language acceptance criteria met
  - [ ] §27.2 Compiler acceptance criteria met
  - [ ] §27.3 Runtime acceptance criteria met
  - [ ] §27.4 Reflection acceptance criteria met
  - [ ] §27.5 RPC acceptance criteria met
  - [ ] §27.6 Compatibility acceptance criteria met
  - [ ] §27.7 Tooling acceptance criteria met
  - [ ] §27.8 Security acceptance criteria met
  - [ ] §27.9 Documentation acceptance criteria met

---

## Phase 21 — Stretch / Post-v1: Polyglot Codegen & Interop
(`tpt20-codegen-backends`, spec §3.4, §22.5)

*Not required for v1. Rust is the reference implementation; this phase is deferred until
the core system (Phases 0–20) is complete and stable.*

- [ ] Multi-language codegen framework driven by the neutral IR
- [ ] Go code generator (where feasible)
- [ ] Go minimal runtime (where feasible)
- [ ] Java code generator (where feasible)
- [ ] Java minimal runtime (where feasible)
- [ ] Python code generator (where feasible)
- [ ] Python minimal runtime (where feasible)
- [ ] Cross-language interop tests: Go clients/servers
- [ ] Cross-language interop tests: Java clients/servers
- [ ] Cross-language interop tests: Python clients/servers
- [ ] Interop tests: HTTP/2 proxies
- [ ] Interop tests: load balancers

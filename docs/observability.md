# Observability

`runtime/tpt20-observability` defines the metrics, tracing, and structured
logging *contracts* used by the RPC runtime and transport layers (spec §19).
It deliberately contains no backend implementation — you plug in a real
metrics/tracing/logging system by implementing the traits or reading the
constants it defines.

## Metrics

Implement the `Metrics` trait to receive callbacks at well-defined
instrumentation points, or use `NoopMetrics` (the default) to discard
everything with zero overhead:

```rust
pub trait Metrics: Send + Sync {
    fn requests_started(&self, labels: &Labels);
    fn requests_completed(&self, labels: &Labels);
    fn request_duration(&self, labels: &Labels, duration: Duration);
    fn active_streams(&self, labels: &Labels, delta: i64);
    fn cancelled_requests(&self, labels: &Labels);
    fn deadline_exceeded_requests(&self, labels: &Labels);
    fn bytes_sent(&self, labels: &Labels, bytes: u64);
    fn bytes_received(&self, labels: &Labels, bytes: u64);
    fn messages_sent(&self, labels: &Labels, count: u64);
    fn messages_received(&self, labels: &Labels, count: u64);
    fn decode_failures(&self, labels: &Labels);
    fn encode_failures(&self, labels: &Labels);
    fn connection_errors(&self, labels: &Labels);
    fn stream_resets(&self, labels: &Labels);
}
```

Every method takes a `Labels` value carrying the shared dimension set (spec
§19.1):

```rust
let labels = Labels::new()
    .service("user.v1.UserService")
    .method("GetUser")
    .status("OK")
    .streaming_type("unary")   // "unary" | "server_streaming" | "client_streaming" | "bidi_streaming"
    .transport("tcp");
```

Wire your backend in globally through the hook registry rather than passing
a `Metrics` implementation through every call site:

```rust
use tpt20_observability::{set_global_metrics, global_metrics};

set_global_metrics(std::sync::Arc::new(MyPrometheusMetrics::new()));
// elsewhere:
global_metrics().requests_started(&labels);
```

Map these directly onto Prometheus counters/histograms or an OpenTelemetry
metrics pipeline; the trait's method names are the metric names from spec
§19.1 (requests started/completed, duration, active streams, cancelled,
deadline-exceeded, bytes/messages sent/received, decode/encode failures,
connection errors, stream resets).

## Tracing

Span attribute names match OpenTelemetry's RPC semantic conventions and are
exposed as constants so you don't have to hand-type them (and risk a typo
that silently breaks dashboards built against the "real" convention):

```rust
pub const RPC_SYSTEM: &str = "rpc.system";              // = RPC_SYSTEM_VALUE ("tpt20")
pub const RPC_SERVICE: &str = "rpc.service";
pub const RPC_METHOD: &str = "rpc.method";
pub const RPC_STATUS: &str = "rpc.status";
pub const RPC_SCHEMA_FINGERPRINT: &str = "rpc.schema_fingerprint";
```

Set these as span attributes when starting/finishing a span around an RPC
call, using whatever tracing crate your application already uses (`tracing`,
OpenTelemetry SDK, etc.):

```rust
let span = tracing::info_span!(
    "rpc",
    { tpt20_observability::RPC_SYSTEM } = tpt20_observability::RPC_SYSTEM_VALUE,
    { tpt20_observability::RPC_SERVICE } = "user.v1.UserService",
    { tpt20_observability::RPC_METHOD } = "GetUser",
);
```

`rpc.schema_fingerprint` is the one addition beyond the standard OTel RPC
conventions — attach the schema fingerprint (see
[Schema language § Schema evolution](schema-language.md#schema-evolution))
for any call so a trace can be correlated back to the exact schema version in
use, independent of the deployed code version.

## Structured logging

`LogEvent` captures the fields every RPC log line should carry, built with a
chained `with`-style API:

```rust
let event = LogEvent::new()
    .request_id("req-123")
    .service("user.v1.UserService")
    .method("GetUser")
    .status("OK")
    .deadline("2026-09-01T01:00:00Z")
    .cancellation_reason("client_cancelled")
    .peer_info("192.168.1.1:443")
    .schema_fingerprint("abcd1234");
```

Every field is `Option`al — a backend should serialize only the fields that
are populated (to JSON, logfmt, or whatever your logging pipeline expects).
`peer_info` is deliberately optional so you can omit it under a privacy
policy that restricts logging client addresses, without changing the
`LogEvent` shape used elsewhere.

## Schema-aware debugging (CLI)

Spec §19.4 calls for `tpt20 decode --schema user.tpt --message user.v1.User`.
The current CLI's `decode` command operates on the raw field model without a
schema (see [CLI reference § decode](cli-reference.md#decode)); schema-aware
inspection today goes through `tpt20 reflect` instead. `tpt20-observability`
itself contains no CLI code — this remains tracked as CLI work, not runtime
work.

## Putting it together

None of the three pillars require the others: you can wire up `Metrics`
without touching tracing, or emit `LogEvent`s without a metrics backend. In
a full deployment, the natural correlation key across all three is
`request_id` (log) / a trace ID (span) plus the `service`/`method` labels
shared by `Labels` and the tracing attribute constants — keep those values
consistent across the three systems so a single request can be traced from
metric spike, to trace span, to log line.

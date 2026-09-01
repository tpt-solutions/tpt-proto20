# Changelog

All notable changes to `tpt20-observability` are documented here.

## [Unreleased]

### Added
- Initial observability crate (Phase 12, spec §19)
- `Metrics` trait with counters, gauges, and histograms
- `Labels` dimensions: service, method, status, streaming_type, transport
- `NoopMetrics` for builds without telemetry
- Tracing span attributes matching OpenTelemetry RPC semantic conventions
- Structured logging via `LogEvent`
- Request ID, service, method, status, deadline, cancellation reason, peer info, schema fingerprint fields

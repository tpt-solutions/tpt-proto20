# Changelog

All notable changes to `tpt20-transport` are documented here.

## [Unreleased]

### Added
- Initial transport layer (Phase 11, spec §17)
- Message framing: 1-byte flags + 4-byte big-endian length + N-byte payload
- Transport traits (transport-agnostic RPC interface)
- In-process transport for tests, embedded systems, local dev, benchmarking, fuzzing
- HTTP/2 transport (feature-gated)
- QUIC/HTTP3 transport (optional, feature-gated)
- Custom stream transport extension point
- Compression-enabled flag in framing
- Reserved bits for future protocol extensions
- TLS with ALPN support
- Cleartext h2c for local development (explicit opt-in)

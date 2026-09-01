# Changelog

All notable changes to `tpt20-compat-grpc` are documented here.

## [Unreleased]

### Added
- Initial gRPC compatibility adapter (Phase 15, spec §10.3)
- HTTP/2 framing compatible with gRPC
- Protobuf-compatible message payload support
- Status code mapping (tpt20 ↔ gRPC)
- Metadata mapping (tpt20 ↔ gRPC)
- Deadline mapping (`grpc-timeout` header)
- Streaming mode mapping (unary / server-stream / client-stream / bidi)
- gRPC message framing (5-byte header with MSB compression flag)
- gRPC health-checking protocol support
- gRPC reflection protocol support

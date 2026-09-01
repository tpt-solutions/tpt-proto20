# Changelog

All notable changes to `tpt20-conformance` are documented here.

## [Unreleased]

### Added
- Initial conformance and testing suite (Phase 17, spec §22)
- Native conformance suite modules: schema parsing, semantic analysis, wire encoding/decoding, canonical encoding, JSON/text mapping, reflection, dynamic messages, RPC behavior, streaming, deadlines, cancellation, security limits
- Compatibility conformance suite: protobuf schema import, protobuf binary decoding/encoding, gRPC RPC behavior, status mapping, metadata mapping, streaming semantics
- Property-based roundtrip tests
- Rust ↔ Rust interoperability test baseline

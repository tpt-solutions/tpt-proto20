# Changelog

All notable changes to `tpt20-ir` are documented here.

## [Unreleased]

### Added
- Initial neutral IR types (Phase 3, spec §8)
- `PackageIr`, `MessageIr`, `FieldIr`, `EnumIr`, `ServiceIr`, `MethodIr`, `OneofIr`
- `SourceSpan` for source locations
- `CompatMetadata` and `ReservedIr`
- Stable schema fingerprint support
- JSON serialization via serde
- Source location metadata for diagnostics

# Changelog

All notable changes to `tpt20-json` are documented here.

## [Unreleased]

### Added
- Initial JSON mapping primitives (Phase 8, spec §14.2)
- `JsonError` type covering parse, base64, type-mismatch, and invalid-enum cases
- `get_field` helper accepting original and lowerCamelCase field names
- 64-bit integer string representation rules on encode and decode
- Bytes fields as base64
- Enum representation by name or by number

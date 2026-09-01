# Changelog

All notable changes to `tpt20-codegen-rust` are documented here.

## [Unreleased]

### Added
- Initial Rust code generator from neutral IR (Phase 5, spec §12)
- Generated owned message structs with `encode` / `decode` / `to_json` / `from_json`
- Generated borrowed view types (`XView<'a>`) for zero-copy access
- Generated Rust enums for schema enums with open/closed unknown-value semantics
- Generated oneof enums as mutually exclusive Rust enums
- Opt-in validated builders with annotation-constraint validation (`@max_len`, `@min_len`, `@range`)
- Codegen options for controlling crate references and builder emission
- `tpt20-cli` `gen rust` wiring

# Changelog

All notable changes to `tpt20-compiler` are documented here.

## [Unreleased]

### Added
- Full compiler pipeline: lexer → parser → AST → semantic analysis → IR → descriptor → fingerprint (Phase 2, spec §7)
- Semantic analysis pass: duplicate ID/name detection, import resolution, oneof/map/annotation validation
- Compatibility-change detector: classifies changes as SAFE / WARNING / BREAKING
- Diagnostics engine with file/line/column/span/severity/code/suggested fix
- Schema fingerprinting from canonical descriptor
- Schema history manifest (`SchemaHistoryManifest`)
- `check` and `compile` public API
- `diff_sources` and `render_report` for schema diffing
- `tpt20-cli` integration for `check`, `fmt`, `lint`, `diff`, `gen`, `descriptors`, `reflect`

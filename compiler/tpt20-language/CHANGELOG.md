# Changelog

All notable changes to `tpt20-language` are documented here.

## [Unreleased]

### Added
- Initial lexer for `.tpt` source (Phase 1, spec §6)
- Initial parser producing `ast::File`
- AST data structures for messages, fields, enums, services, oneofs, maps, annotations, and imports
- Round-trip parse test for spec §6.1 example schema
- `parse_file` convenience wrapper
- Error recovery / helpful parse error messages

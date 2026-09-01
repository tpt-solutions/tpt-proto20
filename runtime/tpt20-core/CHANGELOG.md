# Changelog

All notable changes to `tpt20-core` are documented here.

## [Unreleased]

### Added
- Initial wire format encoder/decoder (Phase 4, spec §9)
- Wire classes: VARINT (0), FIXED32 (1), FIXED64 (2), LEN (3)
- Tag encoding: `tag = (field_id << 3) | wire_class`
- Scalar support: bool, int32, int64, uint32, uint64, sint32, sint64, fixed32, fixed64, sfixed32, sfixed64, float32, float64, string, bytes
- Varint encoding/decoding with zigzag for signed types
- Length-delimited field encoding/decoding
- Repeated field packed/unpacked encoding/decoding
- Map encoding/decoding as repeated synthetic map-entry messages
- Oneof encoding/decoding
- Unknown field handling: preserve / discard / fail
- Canonical deterministic encoding mode
- Optional `Envelope` message
- `DecoderLimits` with max_message_bytes, max_depth, max_field_count, max_unknown_field_bytes, max_string_bytes, max_bytes_field_bytes, max_repeated_entries, max_map_entries
- UTF-8 validation for all string fields
- Checked-arithmetic integer safety throughout encode/decode
- Recursion/depth bounding for nested messages
- `DynamicMessage` for descriptor-driven decoding
- Borrowed view types for zero-copy access

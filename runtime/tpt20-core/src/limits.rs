//! Decoder resource limits (spec §9, `DecoderLimits`).
//!
//! Defaults are deliberately conservative to protect against hostile input:
//! deep recursion, huge messages, excessive field counts, oversized strings
//! and byte fields, and maliciously large repeated/map fields.

/// Bounds enforced on untrusted input during decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderLimits {
    /// Maximum total message size in bytes.
    pub max_message_bytes: usize,
    /// Maximum nesting depth of messages.
    pub max_depth: usize,
    /// Maximum number of fields in a single message.
    pub max_field_count: usize,
    /// Maximum total bytes of preserved unknown fields.
    pub max_unknown_field_bytes: usize,
    /// Maximum bytes for a single string field.
    pub max_string_bytes: usize,
    /// Maximum bytes for a single `bytes` field.
    pub max_bytes_field_bytes: usize,
    /// Maximum entries in a repeated field.
    pub max_repeated_entries: usize,
    /// Maximum entries in a map.
    pub max_map_entries: usize,
}

impl Default for DecoderLimits {
    fn default() -> Self {
        DecoderLimits {
            max_message_bytes: 4 * 1024 * 1024,
            max_depth: 100,
            max_field_count: 32 * 1024,
            max_unknown_field_bytes: 4 * 1024 * 1024,
            max_string_bytes: 4 * 1024 * 1024,
            max_bytes_field_bytes: 16 * 1024 * 1024,
            max_repeated_entries: 512 * 1024,
            max_map_entries: 512 * 1024,
        }
    }
}

/// Policy for handling unknown (unrecognized) fields during decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownFieldPolicy {
    /// Preserve unknown fields so they can be re-encoded later (default).
    Preserve,
    /// Silently discard unknown fields.
    Discard,
    /// Fail decoding if any unknown field is encountered.
    Fail,
}

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

impl DecoderLimits {
    /// Checks that `len` fits within `max_message_bytes`.
    pub fn check_message_bytes(&self, len: usize) -> Result<(), crate::error::DecodeError> {
        if len > self.max_message_bytes {
            Err(crate::error::DecodeError::LimitExceeded {
                limit: self.max_message_bytes,
            })
        } else {
            Ok(())
        }
    }

    /// Checks that `len` fits within `max_string_bytes` (spec §18.1).
    pub fn check_string_bytes(&self, len: usize) -> Result<(), crate::error::DecodeError> {
        if len > self.max_string_bytes {
            Err(crate::error::DecodeError::LimitExceeded {
                limit: self.max_string_bytes,
            })
        } else {
            Ok(())
        }
    }

    /// Bounds nesting depth for recursive (descriptor-driven or generated)
    /// decoders (spec §18.4). `depth` starts at 1 for the outermost message.
    pub fn check_depth(&self, depth: usize) -> Result<(), crate::error::DecodeError> {
        if depth > self.max_depth {
            Err(crate::error::DecodeError::DepthExceeded)
        } else {
            Ok(())
        }
    }

    /// Checks that a repeated field holds at most `max_repeated_entries`.
    pub fn check_repeated_entries(&self, count: usize) -> Result<(), crate::error::DecodeError> {
        if count > self.max_repeated_entries {
            Err(crate::error::DecodeError::RepeatedEntriesExceeded)
        } else {
            Ok(())
        }
    }

    /// Checks that a map holds at most `max_map_entries`.
    pub fn check_map_entries(&self, count: usize) -> Result<(), crate::error::DecodeError> {
        if count > self.max_map_entries {
            Err(crate::error::DecodeError::MapEntriesExceeded)
        } else {
            Ok(())
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

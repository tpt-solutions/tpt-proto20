//! Wire classes and tag encoding for the `tpt20` native wire format (spec §9).
//!
//! The tag scheme is `tag = (field_id << 3) | wire_class`, encoded as a
//! varint. Wire classes determine how the following value is laid out.

/// Wire classes for the native tpt20 wire format.
///
/// Values match the on-the-wire numeric identifiers defined in `spec.txt` §9.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireClass {
    /// Variable-length integer (varint), 7-bit groups, little-endian groups.
    Varint = 0,
    /// 32-bit fixed-width little-endian value.
    Fixed32 = 1,
    /// 64-bit fixed-width little-endian value.
    Fixed64 = 2,
    /// Length-delimited: a varint length followed by that many bytes.
    Len = 3,
}

impl WireClass {
    /// Returns the `WireClass` for a given low 3 bits of a tag, if valid.
    pub fn from_bits(bits: u8) -> Option<WireClass> {
        match bits {
            0 => Some(WireClass::Varint),
            1 => Some(WireClass::Fixed32),
            2 => Some(WireClass::Fixed64),
            3 => Some(WireClass::Len),
            _ => None,
        }
    }
}

/// A decoded wire tag: a field id combined with a wire class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    /// The field identifier (part of the wire contract).
    pub field_id: u32,
    /// The wire class describing the following value layout.
    pub wire_class: WireClass,
}

impl Tag {
    /// Builds a tag from a field id and wire class.
    pub fn new(field_id: u32, wire_class: WireClass) -> Tag {
        Tag {
            field_id,
            wire_class,
        }
    }

    /// Encodes the tag as a `u64` value `field_id << 3 | wire_class`.
    pub fn to_u64(self) -> u64 {
        ((self.field_id as u64) << 3) | (self.wire_class as u8 as u64)
    }

    /// Decodes a tag value into a `(field_id, wire_class)` pair.
    ///
    /// # Errors
    /// Returns [`crate::error::DecodeError::Internal`] if the wire class bits
    /// are not a recognized class.
    pub fn from_u64(value: u64) -> Result<Tag, crate::error::DecodeError> {
        let wire_class = WireClass::from_bits(value as u8 & 0x07)
            .ok_or(crate::error::DecodeError::Internal("unknown wire class"))?;
        let field_id = (value >> 3) as u32;
        Ok(Tag {
            field_id,
            wire_class,
        })
    }
}

//! Descriptor-free dynamic message layer (spec §11.3–11.4).
//!
//! `DynamicMessage` is built on top of the neutral [`RawMessage`] value model.
//! It provides field lookup by id and typed access without requiring
//! compile-time generated types. A full descriptor-driven variant (resolving
//! packed repeated fields, string vs bytes, and oneof membership) is provided
//! once the IR/descriptor model lands; the field model here already preserves
//! all necessary information via the per-field wire class.

use crate::error::DecodeError;
use crate::limits::{DecoderLimits, UnknownFieldPolicy};
use crate::message::{Field, RawMessage, Value};
use crate::scalar;
use crate::wire::WireClass;

/// A message decoded without a compile-time generated type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DynamicMessage {
    raw: RawMessage,
}

impl DynamicMessage {
    /// Creates an empty dynamic message.
    pub fn new() -> DynamicMessage {
        DynamicMessage::default()
    }

    /// Decodes a message from bytes using the supplied limits and policy.
    pub fn decode(
        bytes: &[u8],
        limits: &DecoderLimits,
        policy: UnknownFieldPolicy,
    ) -> Result<DynamicMessage, DecodeError> {
        Ok(DynamicMessage {
            raw: RawMessage::decode(bytes, limits, policy)?,
        })
    }

    /// Encodes the message to a freshly allocated buffer.
    pub fn encode(&self) -> Result<Vec<u8>, crate::error::EncodeError> {
        self.raw.encode()
    }

    /// Encodes the message in canonical/deterministic form.
    pub fn encode_canonical(&self) -> Result<Vec<u8>, crate::error::EncodeError> {
        self.raw.encode_canonical()
    }

    /// Returns the underlying raw field list.
    pub fn fields(&self) -> &[Field] {
        &self.raw.fields
    }

    /// Returns the number of field occurrences.
    pub fn field_count(&self) -> usize {
        self.raw.fields.len()
    }

    /// Iterates over fields with the given id.
    pub fn get(&self, field_id: u32) -> impl Iterator<Item = &Field> {
        self.raw
            .fields
            .iter()
            .filter(move |f| f.field_id == field_id)
    }

    /// Returns the first value for `field_id`, if present.
    pub fn get_first(&self, field_id: u32) -> Option<&Value> {
        self.raw
            .fields
            .iter()
            .find(|f| f.field_id == field_id)
            .map(|f| &f.value)
    }

    /// Reads a string field by id, validating UTF-8.
    pub fn get_string(&self, field_id: u32) -> Result<Option<&str>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_string(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a bytes field by id.
    pub fn get_bytes(&self, field_id: u32) -> Option<&[u8]> {
        self.get_first(field_id)
            .and_then(|v| scalar::decode_bytes(v).ok())
    }

    /// Reads a varint field by id.
    pub fn get_varint(&self, field_id: u32) -> Result<Option<u64>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_uint(v)?)),
            None => Ok(None),
        }
    }

    /// Sets (appends) a field occurrence.
    pub fn set(&mut self, field: Field) {
        self.raw.push(field);
    }

    /// Pushes a varint field.
    pub fn set_varint(&mut self, field_id: u32, value: u64) {
        self.raw.push(Field::new(
            field_id,
            WireClass::Varint,
            Value::Varint(value),
        ));
    }

    /// Pushes a length-delimited (bytes) field.
    pub fn set_bytes(&mut self, field_id: u32, value: &[u8]) {
        self.raw.push(Field::new(
            field_id,
            WireClass::Len,
            Value::Len(value.to_vec()),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_access() {
        let mut m = DynamicMessage::new();
        m.set_varint(1, 99);
        m.set_bytes(2, b"abc");
        assert_eq!(m.get_varint(1).unwrap(), Some(99));
        assert_eq!(m.get_bytes(2), Some(&b"abc"[..]));
        // Asking for a string on a varint field is a type error, not absence.
        assert!(m.get_string(1).is_err());
        let bytes = m.encode().unwrap();
        let back = DynamicMessage::decode(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();
        assert_eq!(m, back);
    }
}

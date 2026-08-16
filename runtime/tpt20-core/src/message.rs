//! Low-level message value model, encoder, and decoder for the native wire
//! format (spec §9, §11).
//!
//! This module provides the building blocks used both by generated code and by
//! the dynamic [`crate::dynamic`] message layer. It operates on a neutral
//! field model so unknown fields can be preserved and re-emitted.

use crate::error::{DecodeError, EncodeError};
use crate::limits::{DecoderLimits, UnknownFieldPolicy};
use crate::varint::{decode_varint, encode_varint, encode_varint_vec};
use crate::wire::{Tag, WireClass};

/// A decoded scalar value in the native wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A varint payload (covers bool, int/uint/sint, enums, sub-field length).
    Varint(u64),
    /// A 32-bit fixed-width little-endian value.
    Fixed32(u32),
    /// A 64-bit fixed-width little-endian value.
    Fixed64(u64),
    /// A length-delimited payload (string, bytes, packed repeated, nested
    /// message, map entry, or unknown length-delimited blob).
    Len(Vec<u8>),
}

/// A single field occurrence on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field identifier.
    pub field_id: u32,
    /// The wire class determined by the tag.
    pub wire_class: WireClass,
    /// The decoded value.
    pub value: Value,
}

impl Field {
    /// Constructs a field from its parts.
    pub fn new(field_id: u32, wire_class: WireClass, value: Value) -> Field {
        Field {
            field_id,
            wire_class,
            value,
        }
    }
}

/// A decoded message as an ordered list of fields. Unknown fields are retained
/// so the message can be re-encoded losslessly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawMessage {
    /// Fields in encounter order (callers may sort for canonical encoding).
    pub fields: Vec<Field>,
}

impl RawMessage {
    /// Creates an empty message.
    pub fn new() -> RawMessage {
        RawMessage::default()
    }

    /// Pushes a field onto the message.
    pub fn push(&mut self, field: Field) {
        self.fields.push(field);
    }

    /// Appends the fields of `other` to this message (used when folding
    /// duplicate map entries / oneof later-wins behavior at higher layers).
    pub fn extend(&mut self, other: RawMessage) {
        self.fields.extend(other.fields);
    }

    /// Decodes a message from `bytes` using the supplied limits and policy.
    pub fn decode(
        bytes: &[u8],
        limits: &DecoderLimits,
        _policy: UnknownFieldPolicy,
    ) -> Result<RawMessage, DecodeError> {
        let mut msg = RawMessage::new();
        decode_message(bytes, limits, &mut msg)?;
        Ok(msg)
    }

    /// Encodes the message to a freshly allocated buffer.
    ///
    /// Field order is preserved as encountered; for canonical/deterministic
    /// output call [`RawMessage::encode_canonical`] after sorting. Length and
    /// varint payloads are emitted in their natural (minimal) form. Unknown
    /// fields are re-emitted as received.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        encode_message(self, &mut out)?;
        Ok(out)
    }

    /// Encodes the message in canonical/deterministic form.
    ///
    /// Fields are ordered by `(field_id, wire_class, payload)` so the output is
    /// suitable for hashing, signing, auditing, and reproducible builds. See
    /// `spec.txt` §9 canonical mode.
    pub fn encode_canonical(&self) -> Result<Vec<u8>, EncodeError> {
        let mut sorted = self.fields.clone();
        sorted.sort_by(|a, b| {
            a.field_id
                .cmp(&b.field_id)
                .then_with(|| (a.wire_class as u8).cmp(&(b.wire_class as u8)))
                .then_with(|| canonical_payload(a).cmp(&canonical_payload(b)))
        });
        let msg = RawMessage { fields: sorted };
        msg.encode()
    }
}

fn canonical_payload(f: &Field) -> Vec<u8> {
    match &f.value {
        Value::Varint(v) => encode_varint_vec_canon(*v),
        Value::Fixed32(v) => v.to_le_bytes().to_vec(),
        Value::Fixed64(v) => v.to_le_bytes().to_vec(),
        Value::Len(v) => v.clone(),
    }
}

fn encode_varint_vec_canon(value: u64) -> Vec<u8> {
    encode_varint_vec(value)
}

fn decode_message(
    bytes: &[u8],
    limits: &DecoderLimits,
    out: &mut RawMessage,
) -> Result<(), DecodeError> {
    let mut cursor = 0usize;
    let mut field_count = 0usize;
    while cursor < bytes.len() {
        if field_count >= limits.max_field_count {
            return Err(DecodeError::FieldCountExceeded);
        }
        let tag_value = decode_varint(bytes, &mut cursor)?;
        let tag = Tag::from_u64(tag_value)?;
        let field_id = tag.field_id;
        let value = match tag.wire_class {
            WireClass::Varint => {
                let v = decode_varint(bytes, &mut cursor)?;
                Value::Varint(v)
            }
            WireClass::Fixed32 => {
                let bytes_read = read_fixed(bytes, &mut cursor, 4)?;
                Value::Fixed32(u32::from_le_bytes(bytes_read[..4].try_into().unwrap()))
            }
            WireClass::Fixed64 => {
                let bytes_read = read_fixed(bytes, &mut cursor, 8)?;
                Value::Fixed64(u64::from_le_bytes(bytes_read))
            }
            WireClass::Len => {
                let len = decode_varint(bytes, &mut cursor)? as usize;
                if cursor + len > bytes.len() {
                    return Err(DecodeError::Truncated);
                }
                if len > limits.max_bytes_field_bytes {
                    return Err(DecodeError::LimitExceeded {
                        limit: limits.max_bytes_field_bytes,
                    });
                }
                let payload = bytes[cursor..cursor + len].to_vec();
                cursor += len;
                Value::Len(payload)
            }
        };
        out.push(Field::new(field_id, tag.wire_class, value));
        field_count += 1;
    }
    Ok(())
}

fn read_fixed(bytes: &[u8], cursor: &mut usize, n: usize) -> Result<[u8; 8], DecodeError> {
    if *cursor + n > bytes.len() {
        return Err(DecodeError::Truncated);
    }
    let mut buf = [0u8; 8];
    buf[..n].copy_from_slice(&bytes[*cursor..*cursor + n]);
    *cursor += n;
    Ok(buf)
}

fn encode_message(msg: &RawMessage, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    for field in &msg.fields {
        let tag = Tag::new(field.field_id, field.wire_class).to_u64();
        encode_varint(tag, out);
        match &field.value {
            Value::Varint(v) => encode_varint(*v, out),
            Value::Fixed32(v) => out.extend_from_slice(&v.to_le_bytes()),
            Value::Fixed64(v) => out.extend_from_slice(&v.to_le_bytes()),
            Value::Len(payload) => {
                encode_varint(payload.len() as u64, out);
                out.extend_from_slice(payload);
            }
        }
    }
    Ok(())
}

/// Re-export so callers can build canonical varints without reaching into the
/// private helper above.
pub use crate::varint::encode_varint_vec as canonical_varint;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::WireClass;

    #[test]
    fn roundtrip_simple() {
        let mut msg = RawMessage::new();
        msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
        msg.push(Field::new(2, WireClass::Len, Value::Len(b"hello".to_vec())));
        msg.push(Field::new(3, WireClass::Fixed64, Value::Fixed64(7)));
        let bytes = msg.encode().unwrap();
        let back = RawMessage::decode(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn canonical_is_deterministic() {
        let mut a = RawMessage::new();
        a.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
        a.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
        let mut b = RawMessage::new();
        b.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
        b.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
        assert_eq!(a.encode_canonical().unwrap(), b.encode_canonical().unwrap());
    }

    #[test]
    fn rejects_truncated_fixed() {
        let bytes = encode_varint_vec(Tag::new(1, WireClass::Fixed64).to_u64());
        // Missing the 8 fixed bytes after the tag.
        let res = RawMessage::decode(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        );
        assert_eq!(res, Err(DecodeError::Truncated));
    }

    #[test]
    fn rejects_overlong_len() {
        let mut bytes = encode_varint_vec(Tag::new(1, WireClass::Len).to_u64());
        bytes.extend_from_slice(&encode_varint_vec(10));
        // Declared length 10 but no payload follows.
        let res = RawMessage::decode(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        );
        assert_eq!(res, Err(DecodeError::Truncated));
    }
}

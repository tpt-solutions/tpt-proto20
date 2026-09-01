//! Protobuf wire format encode/decode adapter (spec §10.2).
//!
//! Implements encoding and decoding of protobuf-compatible binary messages,
//! translating between protobuf wire types and the native tpt20-core wire model.
//!
//! Protobuf wire type mapping:
//!   protobuf 0 (varint)       ↔ tpt20 Varint
//!   protobuf 1 (64-bit)       ↔ tpt20 Fixed64
//!   protobuf 2 (length-delim) ↔ tpt20 Len
//!   protobuf 5 (32-bit)       ↔ tpt20 Fixed32
//!
//! Groups (wire types 3 and 4) are not supported by tpt20-core and are
//! rejected during decode.

use crate::error::WireError;
use tpt20_core::{
    message::{Field, RawMessage, Value},
    wire::{Tag, WireClass},
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Decodes a protobuf-compatible binary message into a tpt20 `RawMessage`.
///
/// This is the protobuf counterpart of `tpt20-core`'s native decode: it
/// translates protobuf wire types into the native wire class model.
pub fn decode_protobuf(bytes: &[u8]) -> Result<RawMessage, WireError> {
    decode_protobuf_with(bytes, &Default::default())
}

/// Decodes with explicit decoder limits.
pub fn decode_protobuf_with(
    bytes: &[u8],
    limits: &tpt20_core::limits::DecoderLimits,
) -> Result<RawMessage, WireError> {
    limits
        .check_message_bytes(bytes.len())
        .map_err(|_| WireError::Internal("limit exceeded"))?;

    let mut msg = RawMessage::new();
    let mut cursor = 0usize;
    let mut field_count = 0usize;

    while cursor < bytes.len() {
        if field_count >= limits.max_field_count {
            return Err(WireError::Internal("field count exceeded"));
        }
        let tag_value = tpt20_core::varint::decode_varint(bytes, &mut cursor)
            .map_err(|_| WireError::VarintOverflow)?;
        let tag = from_protobuf_tag(tag_value)?;

        let value = match tag.wire_class {
            WireClass::Varint => {
                let v = tpt20_core::varint::decode_varint(bytes, &mut cursor)
                    .map_err(|_| WireError::VarintOverflow)?;
                Value::Varint(v)
            }
            WireClass::Fixed32 => {
                let b = read_fixed(bytes, &mut cursor, 4)?;
                Value::Fixed32(u32::from_le_bytes(b[..4].try_into().expect("4 bytes")))
            }
            WireClass::Fixed64 => {
                let b = read_fixed(bytes, &mut cursor, 8)?;
                Value::Fixed64(u64::from_le_bytes(b))
            }
            WireClass::Len => {
                let payload = split_len_delimited(bytes, &mut cursor, limits)?;
                Value::Len(payload.to_vec())
            }
        };

        field_count += 1;
        msg.push(Field::new(tag.field_id, tag.wire_class, value));
    }

    Ok(msg)
}

/// Encodes a tpt20 `RawMessage` into protobuf-compatible binary.
///
/// Translates native wire classes to protobuf wire types.
pub fn encode_protobuf(msg: &RawMessage) -> Result<Vec<u8>, WireError> {
    encode_protobuf_with(msg, &Default::default())
}

/// Encodes with explicit encoder context (no additional limits beyond what
/// `RawMessage::encode` enforces).
pub fn encode_protobuf_with(
    msg: &RawMessage,
    _limits: &tpt20_core::limits::DecoderLimits,
) -> Result<Vec<u8>, WireError> {
    let mut out = Vec::new();
    for field in &msg.fields {
        let tag = to_protobuf_tag(field.field_id, field.wire_class)
            .map_err(|_| WireError::Internal("unsupported wire class for protobuf"))?;
        tpt20_core::varint::encode_varint(tag, &mut out);
        match &field.value {
            Value::Varint(v) => {
                tpt20_core::varint::encode_varint(*v, &mut out);
            }
            Value::Fixed32(v) => {
                out.extend_from_slice(&v.to_le_bytes());
            }
            Value::Fixed64(v) => {
                out.extend_from_slice(&v.to_le_bytes());
            }
            Value::Len(b) => {
                tpt20_core::varint::encode_varint(b.len() as u64, &mut out);
                out.extend_from_slice(b);
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Wire type translation
// ---------------------------------------------------------------------------

/// Converts a protobuf tag value (varint) into a tpt20 `Tag`, mapping wire types.
fn from_protobuf_tag(value: u64) -> Result<Tag, WireError> {
    let proto_wire = (value & 0x07) as u8;
    let field_id = (value >> 3) as u32;
    let wire_class = match proto_wire {
        0 => WireClass::Varint,
        1 => WireClass::Fixed64,
        2 => WireClass::Len,
        3 | 4 => return Err(WireError::GroupWireType),
        5 => WireClass::Fixed32,
        _ => return Err(WireError::UnsupportedWireType(proto_wire)),
    };
    Ok(Tag::new(field_id, wire_class))
}

/// Converts a tpt20 `Tag` into a protobuf tag value.
fn to_protobuf_tag(field_id: u32, wire_class: WireClass) -> Result<u64, WireError> {
    let proto_wire = match wire_class {
        WireClass::Varint => 0u8,
        WireClass::Fixed64 => 1u8,
        WireClass::Len => 2u8,
        WireClass::Fixed32 => 5u8,
    };
    Ok(((field_id as u64) << 3) | (proto_wire as u64))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_fixed(bytes: &[u8], cursor: &mut usize, n: usize) -> Result<[u8; 8], WireError> {
    if *cursor + n > bytes.len() {
        return Err(WireError::Truncated);
    }
    let mut buf = [0u8; 8];
    buf[..n].copy_from_slice(&bytes[*cursor..*cursor + n]);
    *cursor += n;
    Ok(buf)
}

fn split_len_delimited<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    limits: &tpt20_core::limits::DecoderLimits,
) -> Result<&'a [u8], WireError> {
    let len = tpt20_core::varint::decode_varint(bytes, cursor).map_err(|_| WireError::Truncated)?
        as usize;
    if *cursor + len > bytes.len() {
        return Err(WireError::Truncated);
    }
    if len > limits.max_message_bytes {
        return Err(WireError::Internal("length-delimited payload too large"));
    }
    let slice = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt20_core::message::{Field, RawMessage, Value};
    use tpt20_core::wire::WireClass;

    #[test]
    fn roundtrip_varint() {
        let mut msg = RawMessage::new();
        msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
        let bytes = encode_protobuf(&msg).unwrap();
        let back = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn roundtrip_fixed32() {
        let mut msg = RawMessage::new();
        msg.push(Field::new(
            5,
            WireClass::Fixed32,
            Value::Fixed32(0x12345678),
        ));
        let bytes = encode_protobuf(&msg).unwrap();
        let back = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn roundtrip_fixed64() {
        let mut msg = RawMessage::new();
        msg.push(Field::new(
            3,
            WireClass::Fixed64,
            Value::Fixed64(0x1122334455667788),
        ));
        let bytes = encode_protobuf(&msg).unwrap();
        let back = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn roundtrip_len() {
        let mut msg = RawMessage::new();
        msg.push(Field::new(1, WireClass::Len, Value::Len(b"hello".to_vec())));
        let bytes = encode_protobuf(&msg).unwrap();
        let back = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn known_golden_vector_varint() {
        // field 1, varint, value 150
        // tag = (1 << 3) | 0 = 8
        // 150 in varint = 0x96 0x01
        let bytes = [0x08, 0x96, 0x01];
        let msg = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg.fields.len(), 1);
        assert_eq!(msg.fields[0].field_id, 1);
        assert_eq!(msg.fields[0].wire_class, WireClass::Varint);
        assert_eq!(msg.fields[0].value, Value::Varint(150));

        let reencoded = encode_protobuf(&msg).unwrap();
        assert_eq!(reencoded, &bytes[..]);
    }

    #[test]
    fn known_golden_vector_32bit() {
        // field 5, fixed32, value 0x01020304
        // tag = (5 << 3) | 5 = 45
        // 45 in varint = 0x2d
        // payload = 0x04 0x03 0x02 0x01 (little-endian)
        let bytes = [0x2d, 0x04, 0x03, 0x02, 0x01];
        let msg = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg.fields.len(), 1);
        assert_eq!(msg.fields[0].field_id, 5);
        assert_eq!(msg.fields[0].wire_class, WireClass::Fixed32);
        assert_eq!(msg.fields[0].value, Value::Fixed32(0x01020304));

        let reencoded = encode_protobuf(&msg).unwrap();
        assert_eq!(reencoded, &bytes[..]);
    }

    #[test]
    fn known_golden_vector_64bit() {
        // field 1, fixed64, value 0x1122334455667788
        // tag = (1 << 3) | 1 = 9
        // 9 in varint = 0x09
        // payload = little-endian
        let bytes = [0x09, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11];
        let msg = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg.fields.len(), 1);
        assert_eq!(msg.fields[0].field_id, 1);
        assert_eq!(msg.fields[0].wire_class, WireClass::Fixed64);
        assert_eq!(msg.fields[0].value, Value::Fixed64(0x1122334455667788));

        let reencoded = encode_protobuf(&msg).unwrap();
        assert_eq!(reencoded, &bytes[..]);
    }

    #[test]
    fn known_golden_vector_len() {
        // field 2, len-delimited, value "testing"
        // tag = (2 << 3) | 2 = 18
        // 18 in varint = 0x12
        // length = 7
        let bytes = [0x12, 0x07, b't', b'e', b's', b't', b'i', b'n', b'g'];
        let msg = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg.fields.len(), 1);
        assert_eq!(msg.fields[0].field_id, 2);
        assert_eq!(msg.fields[0].wire_class, WireClass::Len);
        assert_eq!(msg.fields[0].value, Value::Len(b"testing".to_vec()));

        let reencoded = encode_protobuf(&msg).unwrap();
        assert_eq!(reencoded, &bytes[..]);
    }

    #[test]
    fn negative_varint() {
        // varint for -1 = zigzag(0) = 0x01
        let bytes = [0x08, 0x01];
        let msg = decode_protobuf(&bytes).unwrap();
        assert_eq!(msg.fields[0].value, Value::Varint(1));
        let reencoded = encode_protobuf(&msg).unwrap();
        assert_eq!(reencoded, &bytes[..]);
    }
}

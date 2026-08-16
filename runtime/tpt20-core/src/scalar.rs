//! Typed scalar encode/decode helpers bridging Rust values and the native wire
//! [`Value`](crate::message::Value) model (spec §9 scalar type support).
//!
//! These helpers are used by generated code and the dynamic layer. They
//! centralize zigzag, sign-extension, fixed-width, and UTF-8 validation rules.

use crate::error::DecodeError;
use crate::message::Value;
use crate::varint::{decode_zigzag, encode_zigzag};

/// The wire representation class a scalar type maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarRepr {
    /// Varint carrying an unsigned or boolean or enum value.
    Varint,
    /// Varint carrying a zigzag-encoded signed integer.
    Zigzag,
    /// Varint carrying a sign-extended signed integer (int32/int64).
    SignedVarint,
    /// 32-bit fixed little-endian.
    Fixed32,
    /// 64-bit fixed little-endian.
    Fixed64,
    /// Length-delimited UTF-8 string.
    String,
    /// Length-delimited raw bytes.
    Bytes,
}

/// Encodes a `u64` as a varint value.
pub fn encode_uint(v: u64) -> Value {
    Value::Varint(v)
}

/// Encodes an `i64` with zigzag encoding.
pub fn encode_sint(v: i64) -> Value {
    Value::Varint(encode_zigzag(v))
}

/// Encodes a signed integer using sign-extended varint encoding (int32/int64).
pub fn encode_signed(v: i64) -> Value {
    Value::Varint(v as u64)
}

/// Encodes a `u32` as a fixed32 value.
pub fn encode_fixed32(v: u32) -> Value {
    Value::Fixed32(v)
}

/// Encodes a `u64` as a fixed64 value.
pub fn encode_fixed64(v: u64) -> Value {
    Value::Fixed64(v)
}

/// Encodes a byte string as a length-delimited value.
pub fn encode_bytes(bytes: &[u8]) -> Value {
    Value::Len(bytes.to_vec())
}

/// Encodes a string as a length-delimited, UTF-8 validated value.
///
/// Strings are always valid UTF-8 by construction in Rust, so this never
/// fails for a `&str` input.
pub fn encode_string(s: &str) -> Value {
    Value::Len(s.as_bytes().to_vec())
}

/// Decodes a varint value as an unsigned integer.
pub fn decode_uint(value: &Value) -> Result<u64, DecodeError> {
    match value {
        Value::Varint(v) => Ok(*v),
        _ => Err(DecodeError::Internal("expected varint")),
    }
}

/// Decodes a zigzag value as a signed integer.
pub fn decode_sint(value: &Value) -> Result<i64, DecodeError> {
    match value {
        Value::Varint(v) => Ok(decode_zigzag(*v)),
        _ => Err(DecodeError::Internal("expected varint")),
    }
}

/// Decodes a sign-extended varint value as a signed integer.
pub fn decode_signed(value: &Value) -> Result<i64, DecodeError> {
    match value {
        Value::Varint(v) => Ok(*v as i64),
        _ => Err(DecodeError::Internal("expected varint")),
    }
}

/// Decodes a fixed32 value.
pub fn decode_fixed32(value: &Value) -> Result<u32, DecodeError> {
    match value {
        Value::Fixed32(v) => Ok(*v),
        _ => Err(DecodeError::Internal("expected fixed32")),
    }
}

/// Decodes a fixed64 value.
pub fn decode_fixed64(value: &Value) -> Result<u64, DecodeError> {
    match value {
        Value::Fixed64(v) => Ok(*v),
        _ => Err(DecodeError::Internal("expected fixed64")),
    }
}

/// Decodes a length-delimited value, validating UTF-8 for string fields.
pub fn decode_bytes(value: &Value) -> Result<&[u8], DecodeError> {
    match value {
        Value::Len(b) => Ok(b),
        _ => Err(DecodeError::Internal("expected length-delimited")),
    }
}

/// Decodes a length-delimited value as a UTF-8 string.
pub fn decode_string(value: &Value) -> Result<&str, DecodeError> {
    match value {
        Value::Len(b) => std::str::from_utf8(b).map_err(|_| DecodeError::InvalidUtf8),
        _ => Err(DecodeError::Internal("expected length-delimited")),
    }
}

/// Decodes a fixed32 value interpreted as an `f32`.
pub fn decode_float32(value: &Value) -> Result<f32, DecodeError> {
    Ok(f32::from_bits(decode_fixed32(value)?))
}

/// Encodes an `f32` as a fixed32 value.
pub fn encode_float32(v: f32) -> Value {
    Value::Fixed32(v.to_bits())
}

/// Decodes a fixed64 value interpreted as an `f64`.
pub fn decode_float64(value: &Value) -> Result<f64, DecodeError> {
    Ok(f64::from_bits(decode_fixed64(value)?))
}

/// Encodes an `f64` as a fixed64 value.
pub fn encode_float64(v: f64) -> Value {
    Value::Fixed64(v.to_bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sint_roundtrip() {
        for v in [i64::MIN, -1, 0, 1, i64::MAX] {
            assert_eq!(decode_sint(&encode_sint(v)).unwrap(), v);
        }
    }

    #[test]
    fn string_utf8_validation() {
        let v = encode_string("héllo");
        assert_eq!(decode_string(&v).unwrap(), "héllo");
        let bad = Value::Len(vec![0xff, 0xfe]);
        assert_eq!(decode_string(&bad), Err(DecodeError::InvalidUtf8));
    }

    #[test]
    fn float_roundtrip() {
        for v in [0.0f64, -1.5, 123.456, f64::INFINITY] {
            assert_eq!(decode_float64(&encode_float64(v)).unwrap(), v);
        }
        for v in [0.0f32, -1.5, 9.75] {
            assert_eq!(decode_float32(&encode_float32(v)).unwrap(), v);
        }
    }
}

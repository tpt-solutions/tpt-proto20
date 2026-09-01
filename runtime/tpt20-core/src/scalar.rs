//! Typed scalar encode/decode helpers bridging Rust values and the native wire
//! [`Value`](crate::message::Value) model (spec §9 scalar type support).
//!
//! These helpers are used by generated code and the dynamic layer. They
//! centralize zigzag, sign-extension, fixed-width, and UTF-8 validation rules.

use crate::error::DecodeError;
use crate::message::{BorrowedValue, Value};
use crate::varint::{decode_varint, decode_zigzag, encode_varint, encode_zigzag};

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

/// Decodes a length-delimited value as a UTF-8 string, enforcing
/// `max_string_bytes` (spec §18.1).
pub fn decode_string_limited<'a>(
    value: &'a Value,
    limits: &crate::limits::DecoderLimits,
) -> Result<&'a str, DecodeError> {
    let s = decode_string(value)?;
    limits.check_string_bytes(s.len())?;
    Ok(s)
}

/// Encodes numeric values as one packed repeated varint field (spec §9.6).
pub fn encode_packed_varints(values: &[u64]) -> Value {
    let mut out = Vec::with_capacity(values.len());
    for v in values {
        encode_varint(*v, &mut out);
    }
    Value::Len(out)
}

/// Decodes a packed repeated varint field, enforcing `max_repeated_entries`.
/// Initial allocation is bounded by input length (each element needs >= 1 byte).
pub fn decode_packed_varints(
    value: &Value,
    limits: &crate::limits::DecoderLimits,
) -> Result<Vec<u64>, DecodeError> {
    let payload = decode_bytes(value)?;
    if payload.len() > limits.max_repeated_entries {
        return Err(DecodeError::RepeatedEntriesExceeded);
    }
    let mut out = Vec::with_capacity(payload.len());
    let mut cursor = 0usize;
    while cursor < payload.len() {
        out.push(decode_varint(payload, &mut cursor)?);
        limits.check_repeated_entries(out.len())?;
    }
    Ok(out)
}

/// Encodes `u32` values as one packed repeated fixed32 field (spec §9.6).
pub fn encode_packed_fixed32(values: &[u32]) -> Value {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Value::Len(out)
}

/// Decodes a packed repeated fixed32 field, enforcing `max_repeated_entries`.
pub fn decode_packed_fixed32(
    value: &Value,
    limits: &crate::limits::DecoderLimits,
) -> Result<Vec<u32>, DecodeError> {
    let payload = decode_bytes(value)?;
    if payload.len() / 4 > limits.max_repeated_entries {
        return Err(DecodeError::RepeatedEntriesExceeded);
    }
    if payload.len() % 4 != 0 {
        return Err(DecodeError::MalformedScalar);
    }
    Ok(payload
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes")))
        .collect())
}

/// Encodes `u64` values as one packed repeated fixed64 field (spec §9.6).
pub fn encode_packed_fixed64(values: &[u64]) -> Value {
    let mut out = Vec::with_capacity(values.len() * 8);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Value::Len(out)
}

/// Decodes a packed repeated fixed64 field, enforcing `max_repeated_entries`.
pub fn decode_packed_fixed64(
    value: &Value,
    limits: &crate::limits::DecoderLimits,
) -> Result<Vec<u64>, DecodeError> {
    let payload = decode_bytes(value)?;
    if payload.len() / 8 > limits.max_repeated_entries {
        return Err(DecodeError::RepeatedEntriesExceeded);
    }
    if payload.len() % 8 != 0 {
        return Err(DecodeError::MalformedScalar);
    }
    Ok(payload
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().expect("8 bytes")))
        .collect())
}

/// Decodes a packed repeated varint field from a borrowed value.
pub fn decode_packed_varints_borrowed(
    value: &BorrowedValue,
    limits: &crate::limits::DecoderLimits,
) -> Result<Vec<u64>, DecodeError> {
    let payload = decode_bytes_borrowed(value)?;
    if payload.len() > limits.max_repeated_entries {
        return Err(DecodeError::RepeatedEntriesExceeded);
    }
    let mut out = Vec::with_capacity(payload.len());
    let mut cursor = 0usize;
    while cursor < payload.len() {
        out.push(decode_varint(payload, &mut cursor)?);
        limits.check_repeated_entries(out.len())?;
    }
    Ok(out)
}

/// Decodes a packed repeated fixed32 field from a borrowed value.
pub fn decode_packed_fixed32_borrowed(
    value: &BorrowedValue,
    limits: &crate::limits::DecoderLimits,
) -> Result<Vec<u32>, DecodeError> {
    let payload = decode_bytes_borrowed(value)?;
    if payload.len() / 4 > limits.max_repeated_entries {
        return Err(DecodeError::RepeatedEntriesExceeded);
    }
    if payload.len() % 4 != 0 {
        return Err(DecodeError::MalformedScalar);
    }
    Ok(payload
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes")))
        .collect())
}

/// Decodes a packed repeated fixed64 field from a borrowed value.
pub fn decode_packed_fixed64_borrowed(
    value: &BorrowedValue,
    limits: &crate::limits::DecoderLimits,
) -> Result<Vec<u64>, DecodeError> {
    let payload = decode_bytes_borrowed(value)?;
    if payload.len() / 8 > limits.max_repeated_entries {
        return Err(DecodeError::RepeatedEntriesExceeded);
    }
    if payload.len() % 8 != 0 {
        return Err(DecodeError::MalformedScalar);
    }
    Ok(payload
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().expect("8 bytes")))
        .collect())
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

/// Decodes a varint payload as a `u64` from a borrowed value.
pub fn decode_uint_borrowed(value: &BorrowedValue) -> Result<u64, DecodeError> {
    match value {
        BorrowedValue::Varint(v) => Ok(*v),
        _ => Err(DecodeError::Internal("expected varint")),
    }
}

/// Decodes a zigzag-encoded varint payload as an `i64` from a borrowed value.
pub fn decode_sint_borrowed(value: &BorrowedValue) -> Result<i64, DecodeError> {
    match value {
        BorrowedValue::Varint(v) => Ok(decode_zigzag(*v)),
        _ => Err(DecodeError::Internal("expected varint")),
    }
}

/// Decodes a sign-extended varint payload as an `i64` from a borrowed value.
pub fn decode_signed_borrowed(value: &BorrowedValue) -> Result<i64, DecodeError> {
    match value {
        BorrowedValue::Varint(v) => Ok(decode_signed_from_u64(*v)),
        _ => Err(DecodeError::Internal("expected varint")),
    }
}

/// Decodes a fixed32 payload as a `u32` from a borrowed value.
pub fn decode_fixed32_borrowed(value: &BorrowedValue) -> Result<u32, DecodeError> {
    match value {
        BorrowedValue::Fixed32(v) => Ok(*v),
        _ => Err(DecodeError::Internal("expected fixed32")),
    }
}

/// Decodes a fixed64 payload as a `u64` from a borrowed value.
pub fn decode_fixed64_borrowed(value: &BorrowedValue) -> Result<u64, DecodeError> {
    match value {
        BorrowedValue::Fixed64(v) => Ok(*v),
        _ => Err(DecodeError::Internal("expected fixed64")),
    }
}

/// Decodes a float32 payload from a borrowed value.
pub fn decode_float32_borrowed(value: &BorrowedValue) -> Result<f32, DecodeError> {
    match value {
        BorrowedValue::Fixed32(v) => Ok(f32::from_bits(*v)),
        _ => Err(DecodeError::Internal("expected fixed32")),
    }
}

/// Decodes a float64 payload from a borrowed value.
pub fn decode_float64_borrowed(value: &BorrowedValue) -> Result<f64, DecodeError> {
    match value {
        BorrowedValue::Fixed64(v) => Ok(f64::from_bits(*v)),
        _ => Err(DecodeError::Internal("expected fixed64")),
    }
}

/// Decodes a length-delimited value as raw bytes from a borrowed value.
pub fn decode_bytes_borrowed<'a>(value: &'a BorrowedValue<'a>) -> Result<&'a [u8], DecodeError> {
    match value {
        BorrowedValue::Len(b) => Ok(b),
        _ => Err(DecodeError::Internal("expected length-delimited")),
    }
}

/// Decodes a length-delimited value as a UTF-8 string from a borrowed value.
pub fn decode_string_borrowed<'a>(value: &'a BorrowedValue<'a>) -> Result<&'a str, DecodeError> {
    match value {
        BorrowedValue::Len(b) => std::str::from_utf8(b).map_err(|_| DecodeError::InvalidUtf8),
        _ => Err(DecodeError::Internal("expected length-delimited")),
    }
}

/// Decodes a length-delimited value as a UTF-8 string, enforcing
/// `max_string_bytes` (spec §18.1), from a borrowed value.
pub fn decode_string_limited_borrowed<'a>(
    value: &'a BorrowedValue<'a>,
    limits: &crate::limits::DecoderLimits,
) -> Result<&'a str, DecodeError> {
    let s = decode_string_borrowed(value)?;
    limits.check_string_bytes(s.len())?;
    Ok(s)
}

fn decode_signed_from_u64(v: u64) -> i64 {
    i64::from_le_bytes(v.to_le_bytes())
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

    #[test]
    fn string_limit_enforced() {
        let limits = crate::limits::DecoderLimits {
            max_string_bytes: 4,
            ..Default::default()
        };
        let v = encode_string("toolongstring");
        assert_eq!(
            decode_string_limited(&v, &limits),
            Err(DecodeError::LimitExceeded { limit: 4 })
        );
        let ok = encode_string("ok");
        assert_eq!(decode_string_limited(&ok, &limits).unwrap(), "ok");
    }

    #[test]
    fn packed_varints_roundtrip_and_limits() {
        let limits = crate::limits::DecoderLimits::default();
        let values: Vec<u64> = vec![0, 1, 127, 128, u64::MAX];
        let packed = encode_packed_varints(&values);
        assert_eq!(decode_packed_varints(&packed, &limits).unwrap(), values);
        // Unpacked occurrences decode through the same helper one value at a time.
        let single = encode_uint(300);
        assert_eq!(decode_packed_varints(&single, &limits), Err(DecodeError::Internal("expected length-delimited")));

        let tight = crate::limits::DecoderLimits {
            max_repeated_entries: 2,
            ..Default::default()
        };
        assert_eq!(
            decode_packed_varints(&packed, &tight),
            Err(DecodeError::RepeatedEntriesExceeded)
        );
    }

    #[test]
    fn packed_fixed_roundtrip_and_malformed() {
        let limits = crate::limits::DecoderLimits::default();
        let v32: Vec<u32> = vec![1, 2, u32::MAX];
        let p32 = encode_packed_fixed32(&v32);
        assert_eq!(decode_packed_fixed32(&p32, &limits).unwrap(), v32);
        let v64: Vec<u64> = vec![7, u64::MAX];
        let p64 = encode_packed_fixed64(&v64);
        assert_eq!(decode_packed_fixed64(&p64, &limits).unwrap(), v64);
        // Trailing partial word is malformed.
        let bad = Value::Len(vec![1, 2, 3]);
        assert_eq!(decode_packed_fixed32(&bad, &limits), Err(DecodeError::MalformedScalar));
        assert_eq!(decode_packed_fixed64(&bad, &limits), Err(DecodeError::MalformedScalar));
    }
}

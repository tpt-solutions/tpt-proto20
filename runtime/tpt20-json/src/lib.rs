//! `tpt20-json`: JSON representation support for tpt20 messages (spec §14.2).
//!
//! This crate provides the shared primitives used by generated code and by the
//! dynamic/reflection layers to convert between tpt20 values and JSON:
//!
//! - [`JsonError`], the error type for all JSON conversions;
//! - [`base64`], standard base64 for `bytes` fields (spec §14.2);
//! - scalar conversion helpers implementing the spec's JSON rules: 64-bit
//!   integers are representable as strings on both encode and decode.
//!
//! Field-name handling (original vs lowerCamelCase), default-value emission,
//! and unknown-field policies are applied by callers; [`get_field`] accepts
//! either spelling when looking up object members.

pub use serde_json::Value;
pub use serde_json as json;
use thiserror::Error;

/// Errors that can occur while converting between tpt20 messages and JSON.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JsonError {
    /// The input was not valid JSON, or serialization failed.
    #[error("json error: {0}")]
    Json(String),

    /// A bytes field contained invalid base64.
    #[error("invalid base64 in bytes field: {0}")]
    Base64(String),

    /// A JSON value had a different type than the schema field requires.
    #[error("type mismatch: expected {expected}")]
    TypeMismatch {
        /// Human-readable name of the expected JSON type.
        expected: &'static str,
    },

    /// An enum name or number was not part of a closed enum.
    #[error("unknown enum value: {0}")]
    InvalidEnum(String),
}

impl From<serde_json::Error> for JsonError {
    fn from(e: serde_json::Error) -> Self {
        JsonError::Json(e.to_string())
    }
}

/// Looks up an object member by any of the accepted spellings (e.g. original
/// schema name and its lowerCamelCase alias). First match wins.
pub fn get_field<'a>(
    obj: &'a serde_json::Map<String, Value>,
    names: &[&str],
) -> Option<&'a Value> {
    names.iter().find_map(|n| obj.get(*n))
}

/// Reads an `i64` from a JSON number or decimal string (spec §14.2: 64-bit
/// integers are representable as strings).
pub fn as_i64(v: &Value) -> Result<i64, JsonError> {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .ok_or(JsonError::TypeMismatch {
                expected: "64-bit integer",
            }),
        Value::String(s) => s.parse::<i64>().map_err(|_| JsonError::TypeMismatch {
            expected: "64-bit integer string",
        }),
        _ => Err(JsonError::TypeMismatch {
            expected: "64-bit integer",
        }),
    }
}

/// Reads a `u64` from a JSON number or decimal string.
pub fn as_u64(v: &Value) -> Result<u64, JsonError> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
            .ok_or(JsonError::TypeMismatch {
                expected: "unsigned 64-bit integer",
            }),
        Value::String(s) => s.parse::<u64>().map_err(|_| JsonError::TypeMismatch {
            expected: "unsigned 64-bit integer string",
        }),
        _ => Err(JsonError::TypeMismatch {
            expected: "unsigned 64-bit integer",
        }),
    }
}

/// Reads an `f64` from a JSON number.
pub fn as_f64(v: &Value) -> Result<f64, JsonError> {
    v.as_f64()
        .ok_or(JsonError::TypeMismatch { expected: "number" })
}

/// Reads a `bool` from a JSON boolean.
pub fn as_bool(v: &Value) -> Result<bool, JsonError> {
    v.as_bool()
        .ok_or(JsonError::TypeMismatch { expected: "boolean" })
}

/// Reads a string slice from a JSON string.
pub fn as_str(v: &Value) -> Result<&str, JsonError> {
    v.as_str()
        .ok_or(JsonError::TypeMismatch { expected: "string" })
}

/// Encodes an `i64` as a JSON value (string form per spec §14.2).
pub fn i64_to_value(v: i64) -> Value {
    Value::String(v.to_string())
}

/// Encodes a `u64` as a JSON value (string form per spec §14.2).
pub fn u64_to_value(v: u64) -> Value {
    Value::String(v.to_string())
}

/// Standard base64 (RFC 4648, with padding) encoding for `bytes` fields.
pub mod base64 {
    use super::JsonError;

    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// Encodes `data` as standard padded base64.
    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    /// Decodes standard padded base64 into bytes.
    pub fn decode(s: &str) -> Result<Vec<u8>, JsonError> {
        fn val(c: u8) -> Result<u32, JsonError> {
            match c {
                b'A'..=b'Z' => Ok((c - b'A') as u32),
                b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
                b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
                b'+' => Ok(62),
                b'/' => Ok(63),
                _ => Err(JsonError::Base64(format!(
                    "invalid character {:?}",
                    c as char
                ))),
            }
        }
        let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        if bytes.len() % 4 != 0 {
            return Err(JsonError::Base64(
                "length must be a multiple of 4".to_string(),
            ));
        }
        let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
        for chunk in bytes.chunks(4) {
            let pad = chunk.iter().filter(|&&c| c == b'=').count();
            if pad > 2 || chunk[..4 - pad].iter().any(|&c| c == b'=') {
                return Err(JsonError::Base64("misplaced padding".to_string()));
            }
            let mut n: u32 = 0;
            for &c in &chunk[..4] {
                if c == b'=' {
                    n <<= 6;
                } else {
                    n = (n << 6) | val(c)?;
                }
            }
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn rfc4648_vectors() {
            assert_eq!(encode(b""), "");
            assert_eq!(encode(b"f"), "Zg==");
            assert_eq!(encode(b"fo"), "Zm8=");
            assert_eq!(encode(b"foo"), "Zm9v");
            assert_eq!(encode(b"foob"), "Zm9vYg==");
            assert_eq!(encode(b"fooba"), "Zm9vYmE=");
            assert_eq!(encode(b"foobar"), "Zm9vYmFy");
            for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
                assert_eq!(decode(&encode(s.as_bytes())).unwrap(), s.as_bytes());
            }
        }

        #[test]
        fn rejects_bad_input() {
            assert!(decode("A").is_err());
            assert!(decode("AB@=").is_err());
            assert!(decode("=AAA").is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn int64_string_roundtrip() {
        let v = i64_to_value(i64::MIN);
        assert_eq!(v, json!(i64::MIN.to_string()));
        assert_eq!(as_i64(&v).unwrap(), i64::MIN);
        // Plain numbers are also accepted on decode.
        assert_eq!(as_i64(&json!(42)).unwrap(), 42);
        assert_eq!(as_u64(&json!(u64::MAX.to_string())).unwrap(), u64::MAX);
        assert!(as_i64(&json!("nope")).is_err());
        assert!(as_bool(&json!(1)).is_err());
    }

    #[test]
    fn get_field_accepts_aliases() {
        let obj = json!({"userId": 1});
        let map = obj.as_object().unwrap();
        assert!(get_field(map, &["user_id", "userId"]).is_some());
        assert!(get_field(map, &["userid"]).is_none());
    }
}


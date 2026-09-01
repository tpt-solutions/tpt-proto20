use serde_json::Value;

/// Converts an `i64` to a `serde_json::Value`, emitting as string when outside
/// the safe JSON integer range.
pub fn i64_to_value(v: i64) -> Value {
    if v >= i64::MIN + 1 && v <= i64::MAX - 1 {
        Value::Number(serde_json::Number::from(v))
    } else {
        Value::String(v.to_string())
    }
}

/// Converts a `u64` to a `serde_json::Value`, emitting as string when outside
/// the safe JSON integer range.
pub fn u64_to_value(v: u64) -> Value {
    if v <= i64::MAX as u64 {
        Value::Number(serde_json::Number::from(v as i64))
    } else {
        Value::String(v.to_string())
    }
}

/// Attempts to read an `i64` from a `serde_json::Value`, accepting both
/// number and string representations.
pub fn as_i64(value: &Value) -> Result<i64, String> {
    match value {
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| format!("invalid i64: {value}")),
        Value::String(s) => s
            .parse::<i64>()
            .map_err(|_| format!("invalid i64 string: {s}")),
        _ => Err(format!("expected i64, got {value}")),
    }
}

/// Attempts to read a `u64` from a `serde_json::Value`, accepting both
/// number and string representations.
pub fn as_u64(value: &Value) -> Result<u64, String> {
    match value {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| format!("invalid u64: {value}")),
        Value::String(s) => s
            .parse::<u64>()
            .map_err(|_| format!("invalid u64 string: {s}")),
        _ => Err(format!("expected u64, got {value}")),
    }
}

/// Looks up a field in a JSON object by trying a list of candidate names.
pub fn get_field<'a>(
    obj: &'a serde_json::Map<String, Value>,
    candidates: &[&str],
) -> Option<&'a Value> {
    candidates.iter().find_map(|name| obj.get(*name))
}

/// Base64 encode/decode helpers.
pub mod base64 {
    use ::base64::Engine;

    /// Encodes bytes to a base64 string.
    pub fn encode(data: &[u8]) -> String {
        ::base64::engine::general_purpose::STANDARD.encode(data)
    }

    /// Decodes a base64 string to bytes.
    pub fn decode(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
        ::base64::engine::general_purpose::STANDARD.decode(encoded)
    }
}

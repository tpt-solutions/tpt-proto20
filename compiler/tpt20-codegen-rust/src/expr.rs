//! Per-context Rust expression builders for each scalar type.
//!
//! All emitted expressions reference the aliases `__core` (`tpt20_core`) and
//! `__scalar` (`tpt20_core::scalar`) established in the generated header, plus
//! the `__json` alias (`tpt20_json`).

/// Wire-class name as referenced in generated code.
pub const CLASS_VARINT: &str = "__core::WireClass::Varint";
pub const CLASS_FIXED32: &str = "__core::WireClass::Fixed32";
pub const CLASS_FIXED64: &str = "__core::WireClass::Fixed64";
pub const CLASS_LEN: &str = "__core::WireClass::Len";

/// `{v}` (a `&T` reference) -> `__core::Value` for singular encoding.
pub fn enc_value(scalar: &str, v: &str) -> String {
    match scalar {
        "bool" => format!("__core::Value::Varint((*{v}) as u64)"),
        "int32" | "sint32" => format!("__core::Value::Varint(i64::from(*{v}) as u64)"),
        "int64" | "sint64" | "uint64" => format!("__core::Value::Varint((*{v}) as u64)"),
        "uint32" => format!("__core::Value::Varint((*{v}) as u64)"),
        "fixed32" => format!("__core::Value::Fixed32(*{v})"),
        "sfixed32" => format!("__core::Value::Fixed32((*{v}) as u32)"),
        "fixed64" => format!("__core::Value::Fixed64(*{v})"),
        "sfixed64" => format!("__core::Value::Fixed64((*{v}) as u64)"),
        "float32" => format!("__core::Value::Fixed32((*{v}).to_bits())"),
        "float64" => format!("__core::Value::Fixed64((*{v}).to_bits())"),
        "string" => format!("__scalar::encode_string({v})"),
        "bytes" => format!("__scalar::encode_bytes({v})"),
        other => unreachable!("not a scalar: {other}"),
    }
}

/// `(&value, limits)` -> `Result<final_type, DecodeError>` for owned decoding.
pub fn dec_owned(scalar: &str, v: &str, l: &str) -> String {
    match scalar {
        "string" => format!("__scalar::decode_string_limited({v}, {l}).map(str::to_string)"),
        "bytes" => format!("__scalar::decode_bytes({v}).map(<[u8]>::to_vec)"),
        other => dec_numeric(other, v, l),
    }
}

/// Same as [`dec_owned`] but borrows string/bytes payloads (view decoding).
pub fn dec_view(scalar: &str, v: &str, l: &str) -> String {
    match scalar {
        "string" => format!("__scalar::decode_string_limited({v}, {l})"),
        "bytes" => format!("__scalar::decode_bytes({v})"),
        other => dec_numeric(other, v, l),
    }
}

fn dec_numeric(scalar: &str, v: &str, _l: &str) -> String {
    match scalar {
        "bool" => format!("__scalar::decode_uint({v}).map(|x| x != 0)"),
        "int32" => format!("__scalar::decode_signed({v}).map(|x| x as i32)"),
        "int64" => format!("__scalar::decode_signed({v})"),
        "uint32" => format!("__scalar::decode_uint({v}).map(|x| x as u32)"),
        "uint64" => format!("__scalar::decode_uint({v})"),
        "sint32" => format!("__scalar::decode_sint({v}).map(|x| x as i32)"),
        "sint64" => format!("__scalar::decode_sint({v})"),
        "fixed32" => format!("__scalar::decode_fixed32({v})"),
        "sfixed32" => format!("__scalar::decode_fixed32({v}).map(|x| x as i32)"),
        "fixed64" => format!("__scalar::decode_fixed64({v})"),
        "sfixed64" => format!("__scalar::decode_fixed64({v}).map(|x| x as i64)"),
        "float32" => format!("__scalar::decode_float32({v})"),
        "float64" => format!("__scalar::decode_float64({v})"),
        other => unreachable!("not a scalar: {other}"),
    }
}

/// `{v}` -> wire word for packed encoding (varint family -> `u64`,
/// fixed families -> `u32`/`u64` words).
pub fn to_wire_word(scalar: &str, v: &str) -> String {
    match scalar {
        "bool" => format!("{v} as u64"),
        "int32" | "sint32" => format!("i64::from({v}) as u64"),
        "int64" | "sint64" => format!("{v} as u64"),
        "uint32" => format!("{v} as u64"),
        "uint64" => v.to_string(),
        "fixed32" => v.to_string(),
        "sfixed32" => format!("{v} as u32"),
        "float32" => format!("{v}.to_bits()"),
        "fixed64" => v.to_string(),
        "sfixed64" => format!("{v} as u64"),
        "float64" => format!("{v}.to_bits()"),
        other => unreachable!("not packable: {other}"),
    }
}

/// Wire word `{x}` -> element value for packed decoding.
pub fn from_wire_word(scalar: &str, x: &str) -> String {
    match scalar {
        "bool" => format!("({x} != 0)"),
        "int32" => format!("({x} as i32)"),
        "int64" => format!("({x} as i64)"),
        "uint32" => format!("({x} as u32)"),
        "uint64" => x.to_string(),
        "fixed32" => x.to_string(),
        "sfixed32" => format!("({x} as i32)"),
        "float32" => format!("f32::from_bits({x})"),
        "fixed64" => x.to_string(),
        "sfixed64" => format!("({x} as i64)"),
        "float64" => format!("f64::from_bits({x})"),
        other => unreachable!("not packable: {other}"),
    }
}

/// `{v}` (a `&T`) -> `__json::Value` (spec §14.2: 64-bit integers as strings,
/// bytes as base64).
pub fn json_to(scalar: &str, v: &str) -> String {
    match scalar {
        "bool" => format!("__json::Value::Bool(*{v})"),
        "int32" | "sint32" => format!("__json::Value::from(*{v})"),
        "int64" | "sint64" => format!("__json::i64_to_value(*{v})"),
        "uint32" | "fixed32" | "sfixed32" => format!("__json::Value::from(*{v})"),
        "uint64" | "fixed64" => format!("__json::u64_to_value(*{v})"),
        "sfixed64" => format!("__json::i64_to_value(*{v})"),
        "float32" | "float64" => format!("__json::Value::from(*{v})"),
        "string" => format!("__json::Value::String({v}.clone())"),
        "bytes" => format!("__json::Value::String(__json::base64::encode({v}))"),
        other => unreachable!("not a scalar: {other}"),
    }
}

/// JSON value expression -> `Result<T, JsonError>`.
pub fn json_from(scalar: &str, v: &str) -> String {
    match scalar {
        "bool" => format!("__json::as_bool({v})"),
        "int32" | "sint32" => format!("__support::as_i32({v})"),
        "int64" | "sint64" => format!("__json::as_i64({v})"),
        "uint32" | "fixed32" | "sfixed32" => format!("__support::as_u32({v})"),
        "uint64" | "fixed64" => format!("__json::as_u64({v})"),
        "sfixed64" => format!("__json::as_i64({v})"),
        "float32" => format!("__json::as_f64({v}).map(|x| x as f32)"),
        "float64" => format!("__json::as_f64({v})"),
        "string" => format!("__json::as_str({v}).map(str::to_string)"),
        "bytes" => format!("__json::base64::decode(__json::as_str({v})?)"),
        other => unreachable!("not a scalar: {other}"),
    }
}

/// Rust field type inside borrowed view structs.
pub fn view_rust_type(scalar: &str) -> &'static str {
    match scalar {
        "string" => "&'a str",
        "bytes" => "&'a [u8]",
        other => crate::scalars::scalar_info(other)
            .map(|i| i.rust)
            .unwrap_or("()"),
    }
}


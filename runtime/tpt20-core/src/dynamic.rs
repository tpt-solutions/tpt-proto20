//! Descriptor-driven dynamic message layer (spec §11.3–11.4).
//!
//! `DynamicMessage` is built on top of the neutral [`RawMessage`] value model
//! and the lightweight [`MessageDescriptor`] type. It provides field lookup by
//! id and name, typed access, mutation, unknown-field access, JSON conversion,
//! and text conversion without requiring compile-time generated types.

use crate::descriptor::{FieldDescriptor, FieldKind, MessageDescriptor, ScalarKind};
use crate::error::{DecodeError, EncodeError};
use crate::limits::{DecoderLimits, UnknownFieldPolicy};
use crate::message::{decode_borrowed_filtered, BorrowedMessage, Field, RawMessage, Value};
use crate::scalar;
use crate::wire::WireClass;

/// JSON conversion error for dynamic messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicJsonError {
    /// A required field was missing.
    MissingField(&'static str),
    /// A JSON value had the wrong type for the target field.
    TypeMismatch {
        /// Field name.
        field: &'static str,
        /// Expected type description.
        expected: &'static str,
    },
    /// A base64 payload was invalid.
    InvalidBase64(String),
    /// A string contained invalid UTF-8.
    InvalidUtf8(String),
    /// A number was out of range.
    NumberOutOfRange(String),
    /// A serde_json error occurred.
    Json(String),
}

impl std::fmt::Display for DynamicJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DynamicJsonError::MissingField(field) => write!(f, "missing field: {field}"),
            DynamicJsonError::TypeMismatch { field, expected } => {
                write!(f, "type mismatch for {field}: expected {expected}")
            }
            DynamicJsonError::InvalidBase64(msg) => write!(f, "invalid base64: {msg}"),
            DynamicJsonError::InvalidUtf8(msg) => write!(f, "invalid utf-8: {msg}"),
            DynamicJsonError::NumberOutOfRange(msg) => write!(f, "number out of range: {msg}"),
            DynamicJsonError::Json(msg) => write!(f, "json error: {msg}"),
        }
    }
}

impl std::error::Error for DynamicJsonError {}

impl From<serde_json::Error> for DynamicJsonError {
    fn from(e: serde_json::Error) -> Self {
        DynamicJsonError::Json(e.to_string())
    }
}

impl From<crate::error::DecodeError> for DynamicJsonError {
    fn from(e: crate::error::DecodeError) -> Self {
        DynamicJsonError::Json(e.to_string())
    }
}

/// A message decoded without a compile-time generated type.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DynamicMessage {
    raw: RawMessage,
    descriptor: Option<MessageDescriptor>,
}

impl DynamicMessage {
    /// Creates an empty dynamic message.
    pub fn new() -> DynamicMessage {
        DynamicMessage::default()
    }

    /// Creates a dynamic message with a descriptor.
    pub fn with_descriptor(descriptor: MessageDescriptor) -> DynamicMessage {
        DynamicMessage {
            raw: RawMessage::new(),
            descriptor: Some(descriptor),
        }
    }

    /// Decodes a message from bytes using the supplied limits and policy,
    /// without a descriptor (structural decode).
    pub fn decode(
        bytes: &[u8],
        limits: &DecoderLimits,
        policy: UnknownFieldPolicy,
    ) -> Result<DynamicMessage, DecodeError> {
        Ok(DynamicMessage {
            raw: RawMessage::decode(bytes, limits, policy)?,
            descriptor: None,
        })
    }

    /// Decodes a message from bytes using a descriptor to drive decode.
    ///
    /// The descriptor is used to:
    /// - Identify known vs unknown fields for the unknown-field policy
    /// - Resolve string vs bytes for length-delimited fields
    /// - Identify packed repeated fields
    /// - Identify nested message fields
    pub fn decode_descriptor(
        descriptor: MessageDescriptor,
        bytes: &[u8],
        limits: &DecoderLimits,
    ) -> Result<DynamicMessage, DecodeError> {
        let is_known = |id: u32| descriptor.is_known(id);
        let raw = RawMessage::decode_filtered(
            bytes,
            limits,
            UnknownFieldPolicy::Preserve,
            &is_known,
        )?;
        Ok(DynamicMessage {
            raw,
            descriptor: Some(descriptor),
        })
    }

    /// Decodes a borrowed (zero-copy) message from bytes using a descriptor.
    ///
    /// Length-delimited payloads reference the original byte slice without
    /// copying, which is ideal for proxy, cache, streaming-pipeline, and
    /// zero-copy-gateway use cases (spec §11.3).
    pub fn decode_borrowed<'a>(
        descriptor: MessageDescriptor,
        bytes: &'a [u8],
        limits: &DecoderLimits,
    ) -> Result<BorrowedMessage<'a>, DecodeError> {
        let is_known = |id: u32| descriptor.is_known(id);
        decode_borrowed_filtered(bytes, limits, UnknownFieldPolicy::Preserve, &is_known)
    }

    /// Encodes the message to a freshly allocated buffer.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        self.raw.encode()
    }

    /// Encodes the message in canonical/deterministic form.
    pub fn encode_canonical(&self) -> Result<Vec<u8>, EncodeError> {
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
        self.raw.fields.iter().filter(move |f| f.field_id == field_id)
    }

    /// Returns the first value for `field_id`, if present.
    pub fn get_first(&self, field_id: u32) -> Option<&Value> {
        self.raw
            .fields
            .iter()
            .find(|f| f.field_id == field_id)
            .map(|f| &f.value)
    }

    /// Looks up a field by name using the descriptor.
    ///
    /// Returns `None` if no descriptor is attached or the field is not found.
    pub fn get_field_by_name(&self, name: &str) -> Option<&FieldDescriptor> {
        self.descriptor.as_ref()?.field_by_name(name)
    }

    /// Iterates over fields with the given name.
    ///
    /// Returns an empty iterator if no descriptor is attached or the field is
    /// not found.
    pub fn get_by_name(&self, name: &str) -> impl Iterator<Item = &Field> {
        let field_id = self.descriptor.as_ref().and_then(|d| d.field_by_name(name)).map(|f| f.id);
        self.raw.fields.iter().filter(move |f| {
            field_id.map_or(false, |id| f.field_id == id)
        })
    }

    /// Returns the first value for a field by name, if present.
    pub fn get_first_by_name(&self, name: &str) -> Option<&Value> {
        let field_id = self.descriptor.as_ref()?.field_by_name(name).map(|f| f.id)?;
        self.get_first(field_id)
    }

    /// Returns the descriptor, if any.
    pub fn descriptor(&self) -> Option<&MessageDescriptor> {
        self.descriptor.as_ref()
    }

    /// Reads a string field by id, validating UTF-8.
    pub fn get_string(&self, field_id: u32) -> Result<Option<&str>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_string(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a string field by name using the descriptor.
    pub fn get_string_by_name(&self, name: &str) -> Result<Option<&str>, DecodeError> {
        let Some(field) = self.descriptor.as_ref().and_then(|d| d.field_by_name(name)) else {
            return Ok(None);
        };
        self.get_string(field.id)
    }

    /// Reads a bytes field by id.
    pub fn get_bytes(&self, field_id: u32) -> Option<&[u8]> {
        self.get_first(field_id)
            .and_then(|v| scalar::decode_bytes(v).ok())
    }

    /// Reads a bytes field by name using the descriptor.
    pub fn get_bytes_by_name(&self, name: &str) -> Option<&[u8]> {
        let Some(field) = self.descriptor.as_ref().and_then(|d| d.field_by_name(name)) else {
            return None;
        };
        self.get_bytes(field.id)
    }

    /// Reads a varint field by id.
    pub fn get_varint(&self, field_id: u32) -> Result<Option<u64>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_uint(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a varint field by name using the descriptor.
    pub fn get_varint_by_name(&self, name: &str) -> Result<Option<u64>, DecodeError> {
        let Some(field) = self.descriptor.as_ref().and_then(|d| d.field_by_name(name)) else {
            return Ok(None);
        };
        self.get_varint(field.id)
    }

    /// Reads a sint (zigzag) field by id.
    pub fn get_sint(&self, field_id: u32) -> Result<Option<i64>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_sint(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a signed (sign-extended) field by id.
    pub fn get_signed(&self, field_id: u32) -> Result<Option<i64>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_signed(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a fixed32 field by id.
    pub fn get_fixed32(&self, field_id: u32) -> Result<Option<u32>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_fixed32(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a fixed64 field by id.
    pub fn get_fixed64(&self, field_id: u32) -> Result<Option<u64>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_fixed64(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a float32 field by id.
    pub fn get_float32(&self, field_id: u32) -> Result<Option<f32>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_float32(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a float64 field by id.
    pub fn get_float64(&self, field_id: u32) -> Result<Option<f64>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_float64(v)?)),
            None => Ok(None),
        }
    }

    /// Reads a packed repeated varint field by id.
    pub fn get_packed_varints(
        &self,
        field_id: u32,
        limits: &DecoderLimits,
    ) -> Result<Option<Vec<u64>>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_packed_varints(v, limits)?)),
            None => Ok(None),
        }
    }

    /// Reads a packed repeated fixed32 field by id.
    pub fn get_packed_fixed32(
        &self,
        field_id: u32,
        limits: &DecoderLimits,
    ) -> Result<Option<Vec<u32>>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_packed_fixed32(v, limits)?)),
            None => Ok(None),
        }
    }

    /// Reads a packed repeated fixed64 field by id.
    pub fn get_packed_fixed64(
        &self,
        field_id: u32,
        limits: &DecoderLimits,
    ) -> Result<Option<Vec<u64>>, DecodeError> {
        match self.get_first(field_id) {
            Some(v) => Ok(Some(scalar::decode_packed_fixed64(v, limits)?)),
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

    /// Pushes a fixed32 field.
    pub fn set_fixed32(&mut self, field_id: u32, value: u32) {
        self.raw.push(Field::new(
            field_id,
            WireClass::Fixed32,
            Value::Fixed32(value),
        ));
    }

    /// Pushes a fixed64 field.
    pub fn set_fixed64(&mut self, field_id: u32, value: u64) {
        self.raw.push(Field::new(
            field_id,
            WireClass::Fixed64,
            Value::Fixed64(value),
        ));
    }

    /// Sets a string field by name using the descriptor.
    ///
    /// If no descriptor is attached or the field is not found, returns `None`.
    pub fn set_string_by_name(&mut self, name: &str, value: &str) -> Option<()> {
        let field = self.descriptor.as_ref()?.field_by_name(name)?;
        self.raw.push(Field::new(
            field.id,
            WireClass::Len,
            Value::Len(value.as_bytes().to_vec()),
        ));
        Some(())
    }

    /// Sets a bytes field by name using the descriptor.
    pub fn set_bytes_by_name(&mut self, name: &str, value: &[u8]) -> Option<()> {
        let field = self.descriptor.as_ref()?.field_by_name(name)?;
        self.raw.push(Field::new(
            field.id,
            WireClass::Len,
            Value::Len(value.to_vec()),
        ));
        Some(())
    }

    /// Sets a varint field by name using the descriptor.
    pub fn set_varint_by_name(&mut self, name: &str, value: u64) -> Option<()> {
        let field = self.descriptor.as_ref()?.field_by_name(name)?;
        self.raw.push(Field::new(
            field.id,
            WireClass::Varint,
            Value::Varint(value),
        ));
        Some(())
    }

    /// Removes all occurrences of a field by id.
    pub fn remove(&mut self, field_id: u32) {
        self.raw.fields.retain(|f| f.field_id != field_id);
    }

    /// Removes all occurrences of a field by name.
    ///
    /// Returns true if any fields were removed.
    pub fn remove_by_name(&mut self, name: &str) -> bool {
        let Some(field) = self.descriptor.as_ref().and_then(|d| d.field_by_name(name).cloned()) else {
            return false;
        };
        let original_len = self.raw.fields.len();
        self.raw.fields.retain(|f| f.field_id != field.id);
        self.raw.fields.len() < original_len
    }

    /// Converts the dynamic message to a JSON string using the descriptor.
    ///
    /// If no descriptor is attached, only the raw fields are emitted with their
    /// numeric ids as keys.
    pub fn to_json(&self) -> Result<String, DynamicJsonError> {
        let obj = self.to_json_value()?;
        Ok(serde_json::to_string_pretty(&obj)?)
    }

    /// Converts the dynamic message to a `serde_json::Value`.
    pub fn to_json_value(&self) -> Result<serde_json::Value, DynamicJsonError> {
        let mut map = serde_json::Map::new();
        for field in &self.raw.fields {
            let key = if let Some(desc) = self.descriptor.as_ref().and_then(|d| d.field_by_id(field.field_id)) {
                desc.name.clone()
            } else {
                field.field_id.to_string()
            };
            let value = field_value_to_json(&field.value)?;
            map.insert(key, value);
        }
        Ok(serde_json::Value::Object(map))
    }

    /// Creates a `DynamicMessage` from a JSON string using the descriptor.
    pub fn from_json(
        descriptor: MessageDescriptor,
        json: &str,
    ) -> Result<DynamicMessage, DynamicJsonError> {
        let value: serde_json::Value = serde_json::from_str(json)?;
        Self::from_json_value(descriptor, value)
    }

    /// Creates a `DynamicMessage` from a `serde_json::Value` using the descriptor.
    pub fn from_json_value(
        descriptor: MessageDescriptor,
        value: serde_json::Value,
    ) -> Result<DynamicMessage, DynamicJsonError> {
        let mut msg = DynamicMessage::with_descriptor(descriptor);
        let obj = value.as_object().ok_or_else(|| DynamicJsonError::TypeMismatch {
            field: "root",
            expected: "object",
        })?;
        for (key, val) in obj {
            let Some(field_desc) = msg.descriptor.as_ref().and_then(|d| d.field_by_name(key)) else {
                continue;
            };
            let field = json_value_to_field(field_desc, val)?;
            msg.raw.fields.push(field);
        }
        Ok(msg)
    }

    /// Converts the dynamic message to the human-readable text format (spec §14.3).
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for field in &self.raw.fields {
            let field_name = if let Some(desc) = self.descriptor.as_ref().and_then(|d| d.field_by_id(field.field_id)) {
                desc.name.clone()
            } else {
                field.field_id.to_string()
            };
            match &field.value {
                Value::Varint(v) => {
                    out.push_str(&field_name);
                    out.push_str(": ");
                    out.push_str(&v.to_string());
                    out.push('\n');
                }
                Value::Fixed32(v) => {
                    out.push_str(&field_name);
                    out.push_str(": ");
                    out.push_str(&v.to_string());
                    out.push('\n');
                }
                Value::Fixed64(v) => {
                    out.push_str(&field_name);
                    out.push_str(": ");
                    out.push_str(&v.to_string());
                    out.push('\n');
                }
                Value::Len(bytes) => {
                    if let Ok(s) = std::str::from_utf8(bytes) {
                        out.push_str(&field_name);
                        out.push_str(": \"");
                        out.push_str(&s.escape_default().to_string());
                        out.push_str("\"\n");
                    } else {
                        out.push_str(&field_name);
                        out.push_str(": [base64 ");
                        out.push_str(&base64_encode(bytes));
                        out.push_str("]\n");
                    }
                }
            }
        }
        out
    }

    /// Returns unknown fields (fields not present in the descriptor).
    ///
    /// If no descriptor is attached, returns an empty list.
    pub fn unknown_fields(&self) -> Vec<&Field> {
        let Some(desc) = self.descriptor.as_ref() else {
            return Vec::new();
        };
        self.raw
            .fields
            .iter()
            .filter(|f| !desc.is_known(f.field_id))
            .collect()
    }

    /// Returns the number of unknown fields.
    pub fn unknown_field_count(&self) -> usize {
        self.unknown_fields().len()
    }
}

fn field_value_to_json(value: &Value) -> Result<serde_json::Value, DynamicJsonError> {
    match value {
        Value::Varint(v) => Ok(serde_json::Value::String(v.to_string())),
        Value::Fixed32(v) => Ok(serde_json::Value::String(v.to_string())),
        Value::Fixed64(v) => Ok(serde_json::Value::String(v.to_string())),
        Value::Len(bytes) => {
            if let Ok(s) = std::str::from_utf8(bytes) {
                Ok(serde_json::Value::String(s.to_string()))
            } else {
                Ok(serde_json::Value::String(base64_encode(bytes)))
            }
        }
    }
}

fn json_value_to_field(
    field: &FieldDescriptor,
    value: &serde_json::Value,
) -> Result<Field, DynamicJsonError> {
    let wire_class = field.wire_class;
    let field_value = match &field.kind {
        FieldKind::Scalar(scalar) => match scalar {
            ScalarKind::Bool => {
                let b = value.as_bool().ok_or_else(|| DynamicJsonError::TypeMismatch {
                    field: "bool",
                    expected: "boolean",
                })?;
                Value::Varint(if b { 1 } else { 0 })
            }
            ScalarKind::Int32 | ScalarKind::Int64 | ScalarKind::Sint32 | ScalarKind::Sint64 => {
                let i = as_i64(value).map_err(|_| DynamicJsonError::TypeMismatch {
                    field: "int",
                    expected: "integer",
                })?;
                Value::Varint(i as u64)
            }
            ScalarKind::UInt32 | ScalarKind::UInt64 => {
                let u = as_u64(value).map_err(|_| DynamicJsonError::TypeMismatch {
                    field: "uint",
                    expected: "unsigned integer",
                })?;
                Value::Varint(u)
            }
            ScalarKind::Fixed32 | ScalarKind::SFixed32 => {
                let n = value.as_u64().ok_or_else(|| DynamicJsonError::TypeMismatch {
                    field: "fixed32",
                    expected: "unsigned integer",
                })?;
                Value::Fixed32(n as u32)
            }
            ScalarKind::Fixed64 | ScalarKind::SFixed64 => {
                let n = value.as_u64().ok_or_else(|| DynamicJsonError::TypeMismatch {
                    field: "fixed64",
                    expected: "unsigned integer",
                })?;
                Value::Fixed64(n)
            }
            ScalarKind::Float | ScalarKind::Double => {
                let f = value.as_f64().ok_or_else(|| DynamicJsonError::TypeMismatch {
                    field: "float",
                    expected: "number",
                })?;
                match wire_class {
                    WireClass::Fixed32 => Value::Fixed32(f32::to_bits(f as f32)),
                    WireClass::Fixed64 => Value::Fixed64(f64::to_bits(f)),
                    _ => Value::Len(((f as f32).to_le_bytes()).to_vec()),
                }
            }
            ScalarKind::String => {
                let s = value.as_str().ok_or_else(|| DynamicJsonError::TypeMismatch {
                    field: "string",
                    expected: "string",
                })?;
                Value::Len(s.as_bytes().to_vec())
            }
            ScalarKind::Bytes => {
                let s = value.as_str().ok_or_else(|| DynamicJsonError::TypeMismatch {
                    field: "bytes",
                    expected: "base64 string",
                })?;
                let bytes = base64_decode(s).map_err(|e| DynamicJsonError::InvalidBase64(e.to_string()))?;
                Value::Len(bytes)
            }
            ScalarKind::Enum { .. } => {
                let v = if let Some(s) = value.as_str() {
                    s.parse::<i64>().map_err(|_| DynamicJsonError::TypeMismatch {
                        field: "enum",
                        expected: "enum name or number",
                    })?
                } else if let Some(n) = value.as_i64() {
                    n
                } else {
                    return Err(DynamicJsonError::TypeMismatch {
                        field: "enum",
                        expected: "enum name or number",
                    });
                };
                Value::Varint(v as u64)
            }
        },
        FieldKind::Repeated { .. } | FieldKind::Map | FieldKind::Message => {
            let json = serde_json::to_vec(value).map_err(|e| DynamicJsonError::Json(e.to_string()))?;
            Value::Len(json)
        }
    };
    Ok(Field::new(field.id, wire_class, field_value))
}

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid character {:?}", c as char)),
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.len() % 4 != 0 {
        return Err("length must be a multiple of 4".to_string());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        if pad > 2 || chunk[..4 - pad].iter().any(|&c| c == b'=') {
            return Err("misplaced padding".to_string());
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

fn as_i64(v: &serde_json::Value) -> Result<i64, DynamicJsonError> {
    match v {
        serde_json::Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .ok_or(DynamicJsonError::TypeMismatch {
                field: "int",
                expected: "64-bit integer",
            }),
        serde_json::Value::String(s) => s.parse::<i64>().map_err(|_| DynamicJsonError::TypeMismatch {
            field: "int",
            expected: "64-bit integer string",
        }),
        _ => Err(DynamicJsonError::TypeMismatch {
            field: "int",
            expected: "64-bit integer",
        }),
    }
}

fn as_u64(v: &serde_json::Value) -> Result<u64, DynamicJsonError> {
    match v {
        serde_json::Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
            .ok_or(DynamicJsonError::TypeMismatch {
                field: "uint",
                expected: "unsigned 64-bit integer",
            }),
        serde_json::Value::String(s) => s.parse::<u64>().map_err(|_| DynamicJsonError::TypeMismatch {
            field: "uint",
            expected: "unsigned 64-bit integer string",
        }),
        _ => Err(DynamicJsonError::TypeMismatch {
            field: "uint",
            expected: "unsigned 64-bit integer",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DecoderLimits, OneofDescriptor, UnknownFieldPolicy};

    #[test]
    fn dynamic_access() {
        let mut m = DynamicMessage::new();
        m.set_varint(1, 99);
        m.set_bytes(2, b"abc");
        assert_eq!(m.get_varint(1).unwrap(), Some(99));
        assert_eq!(m.get_bytes(2), Some(&b"abc"[..]));
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

    #[test]
    fn descriptor_driven_decode() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(
            1,
            "id",
            WireClass::Varint,
            FieldKind::Scalar(ScalarKind::Int64),
        ));
        desc.add_field(FieldDescriptor::new(
            2,
            "name",
            WireClass::Len,
            FieldKind::Scalar(ScalarKind::String),
        ));
        desc.add_field(FieldDescriptor::new(
            3,
            "tags",
            WireClass::Len,
            FieldKind::Repeated { packed: true },
        ));

        let mut msg = DynamicMessage::with_descriptor(desc.clone());
        msg.set_varint_by_name("id", 42).unwrap();
        msg.set_string_by_name("name", "Ada").unwrap();
        msg.set_bytes_by_name("tags", &[1, 2, 3]).unwrap();

        let bytes = msg.encode().unwrap();
        let back = DynamicMessage::decode_descriptor(desc, &bytes, &DecoderLimits::default()).unwrap();
        assert_eq!(back.get_varint_by_name("id").unwrap(), Some(42));
        assert_eq!(back.get_string_by_name("name").unwrap(), Some("Ada"));
        assert_eq!(back.get_bytes_by_name("tags"), Some(&[1u8, 2, 3][..]));
    }

    #[test]
    fn name_based_lookup() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
        desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));

        let mut msg = DynamicMessage::with_descriptor(desc);
        msg.set_varint_by_name("id", 7).unwrap();
        msg.set_string_by_name("name", "test").unwrap();

        assert!(msg.get_field_by_name("id").is_some());
        assert!(msg.get_field_by_name("name").is_some());
        assert!(msg.get_field_by_name("missing").is_none());
        assert_eq!(msg.get_varint_by_name("id").unwrap(), Some(7));
        assert_eq!(msg.get_string_by_name("name").unwrap(), Some("test"));
    }

    #[test]
    fn unknown_fields_via_descriptor() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));

        let mut msg = DynamicMessage::with_descriptor(desc);
        msg.set_varint(1, 42);
        msg.set_varint(99, 1);

        let unknown = msg.unknown_fields();
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].field_id, 99);
    }

    #[test]
    fn json_roundtrip() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
        desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));

        let mut msg = DynamicMessage::with_descriptor(desc.clone());
        msg.set_varint_by_name("id", 42).unwrap();
        msg.set_string_by_name("name", "Ada").unwrap();

        let json = msg.to_json().unwrap();
        let back = DynamicMessage::from_json(desc, &json).unwrap();
        assert_eq!(back.get_varint_by_name("id").unwrap(), Some(42));
        assert_eq!(back.get_string_by_name("name").unwrap(), Some("Ada"));
    }

    #[test]
    fn text_format_contains_fields() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
        desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));

        let mut msg = DynamicMessage::with_descriptor(desc);
        msg.set_varint_by_name("id", 42).unwrap();
        msg.set_string_by_name("name", "Ada").unwrap();

        let text = msg.to_text();
        assert!(text.contains("id: 42"));
        assert!(text.contains("name: \"Ada\""));
    }

    #[test]
    fn bytes_backed_borrowed_decode() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
        desc.add_field(FieldDescriptor::new(2, "data", WireClass::Len, FieldKind::Scalar(ScalarKind::Bytes)));

        let mut msg = DynamicMessage::with_descriptor(desc.clone());
        msg.set_varint_by_name("id", 1).unwrap();
        msg.set_bytes_by_name("data", b"hello world").unwrap();

        let bytes = msg.encode().unwrap();
        let borrowed = DynamicMessage::decode_borrowed(desc, &bytes, &DecoderLimits::default()).unwrap();

        assert_eq!(borrowed.get_varint(1), Some(1));
        assert_eq!(borrowed.get_bytes(2), Some(&b"hello world"[..]));
    }

    #[test]
    fn borrowed_to_owned_roundtrip() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
        desc.add_field(FieldDescriptor::new(2, "data", WireClass::Len, FieldKind::Scalar(ScalarKind::Bytes)));

        let mut msg = DynamicMessage::with_descriptor(desc.clone());
        msg.set_varint_by_name("id", 7).unwrap();
        msg.set_bytes_by_name("data", b"xyz").unwrap();

        let bytes = msg.encode().unwrap();
        let borrowed = DynamicMessage::decode_borrowed(desc, &bytes, &DecoderLimits::default()).unwrap();
        let owned = borrowed.to_owned();

        assert_eq!(owned, msg.raw);
    }

    #[test]
    fn remove_by_name() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
        desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));

        let mut msg = DynamicMessage::with_descriptor(desc);
        msg.set_varint_by_name("id", 1).unwrap();
        msg.set_string_by_name("name", "test").unwrap();
        assert!(msg.remove_by_name("name"));
        assert!(msg.get_first(2).is_none());
        assert!(!msg.remove_by_name("missing"));
    }

    #[test]
    fn oneof_descriptor() {
        let mut desc = MessageDescriptor::new();
        desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
        desc.add_oneof(OneofDescriptor::new("contact", vec![10, 11]));
        assert_eq!(desc.oneof_members("contact"), Some(&[10, 11][..]));
    }
}

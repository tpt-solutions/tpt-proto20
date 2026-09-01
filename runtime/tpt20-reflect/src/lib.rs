//! Descriptor-driven reflection for tpt20 schemas (spec §13).
//!
//! This crate provides [`DynamicMessage`], a schema-aware dynamic message
//! type built on top of [`tpt20_core::RawMessage`] and
//! [`tpt20_descriptor::Descriptor`]. It enables:
//!
//! - dynamic decoding via descriptor
//! - dynamic encoding via descriptor
//! - field access by name or id
//! - field mutation
//! - repeated field access
//! - map field access
//! - enum access with name resolution
//! - oneof access
//! - nested message access
//! - unknown field access
//! - descriptor lookup
//! - schema fingerprint inspection
//!
//! Example (spec §13):
//!
//! ```rust
//! use tpt20_core::{DecoderLimits, UnknownFieldPolicy};
//! use tpt20_descriptor::Descriptor;
//! use tpt20_reflect::DynamicMessage;
//!
//! let json = r#"{"name":"test.v1","messages":[{"name":"User","fields":[{"id":1,"name":"id","label":{"Singular":{"path":["int64"]}},"presence":"Implicit"},{"id":2,"name":"name","label":{"Singular":{"path":["string"]}},"presence":"Implicit"}],"oneofs":[],"messages":[],"enums":[],"reserved":[],"annotations":[],"span":{"line":1,"column":1}}],"enums":[],"services":[],"reserved":[],"compat":{"policy":"","versions":[],"deprecations":[]},"fingerprint":null}"#;
//! let descriptor = Descriptor::from_json(json).unwrap();
//! let msg = descriptor.find_message("User").unwrap();
//! let message = DynamicMessage::decode(msg, &descriptor, &[], &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
//! let _name = message.get_field("name").unwrap();
//! let _bytes = message.encode().unwrap();
//! ```

use std::borrow::Cow;

use tpt20_core::{
    self, DecodeError, DecoderLimits, EncodeError, Field, RawMessage, UnknownFieldPolicy, Value,
    WireClass,
};
use tpt20_descriptor::Descriptor;
use tpt20_ir as ir;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Scalar name constants (mirrors tpt20-codegen-rust; kept local to avoid
// pulling the codegen crate into the runtime).
// ---------------------------------------------------------------------------

const SCALAR_NAMES: &[&str] = &[
    "bool",
    "int32",
    "int64",
    "uint32",
    "uint64",
    "sint32",
    "sint64",
    "fixed32",
    "sfixed32",
    "fixed64",
    "sfixed64",
    "float32",
    "float64",
    "string",
    "bytes",
];

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors produced by the reflection layer.
#[derive(Debug, Error)]
pub enum ReflectError {
    /// A field with the given name was not found in the message schema.
    #[error("field `{0}` not found in message `{1}`")]
    FieldNotFound(String, String),

    /// A field with the given id was not found in the message schema.
    #[error("field id {0} not found in message `{1}`")]
    FieldIdNotFound(u32, String),

    /// A oneof with the given name was not found in the message schema.
    #[error("oneof `{0}` not found in message `{1}`")]
    OneofNotFound(String, String),

    /// A message with the given name was not found in the descriptor.
    #[error("message `{0}` not found in descriptor")]
    MessageNotFound(String),

    /// A field's value could not be interpreted as the expected schema type.
    #[error("type mismatch for field `{0}`: expected {1}")]
    TypeMismatch(String, &'static str),

    /// A repeated-field operation was applied to a non-repeated field.
    #[error("field `{0}` is not repeated")]
    NotRepeated(String),

    /// A map-field operation was applied to a non-map field.
    #[error("field `{0}` is not a map")]
    NotMap(String),

    /// An enum operation was applied to a non-enum field.
    #[error("field `{0}` is not an enum")]
    NotEnum(String),

    /// A nested-message operation was applied to a non-message field.
    #[error("field `{0}` is not a message")]
    NotMessage(String),

    /// The oneof had no active member on the wire.
    #[error("oneof `{0}` has no active member")]
    OneofNotSet(String),

    /// A decode error from the core wire layer.
    #[error("decode error: {0}")]
    Decode(#[from] DecodeError),

    /// An encode error from the core wire layer.
    #[error("encode error: {0}")]
    Encode(#[from] EncodeError),

    /// A UTF-8 decoding error.
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}

// ---------------------------------------------------------------------------
// Reflected value types
// ---------------------------------------------------------------------------

/// A schema-aware value read from a dynamic message.
#[derive(Debug, Clone, PartialEq)]
pub enum ReflectValue<'a> {
    /// A varint-backed value (bool, int, uint, sint, enum).
    Varint(u64),
    /// A 32-bit fixed-width value (fixed32, sfixed32, float32).
    Fixed32(u32),
    /// A 64-bit fixed-width value (fixed64, sfixed64, float64).
    Fixed64(u64),
    /// A UTF-8 string field.
    String(String),
    /// A raw bytes field.
    Bytes(Vec<u8>),
    /// An enum value with optional name resolved from the descriptor.
    Enum(i32, Option<&'a str>),
    /// A nested message value.
    Message(Box<DynamicMessage<'a>>),
}

impl<'a> ReflectValue<'a> {
    /// Returns the value as a varint if applicable.
    pub fn as_varint(&self) -> Option<u64> {
        match self {
            ReflectValue::Varint(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as a fixed32 if applicable.
    pub fn as_fixed32(&self) -> Option<u32> {
        match self {
            ReflectValue::Fixed32(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as a fixed64 if applicable.
    pub fn as_fixed64(&self) -> Option<u64> {
        match self {
            ReflectValue::Fixed64(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as a string if applicable.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ReflectValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the value as bytes if applicable.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            ReflectValue::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the value as an enum number and name if applicable.
    pub fn as_enum(&self) -> Option<(i32, Option<&str>)> {
        match self {
            ReflectValue::Enum(n, name) => Some((*n, *name)),
            _ => None,
        }
    }

    /// Returns the value as a nested message if applicable.
    pub fn as_message(&self) -> Option<&DynamicMessage<'a>> {
        match self {
            ReflectValue::Message(m) => Some(m),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Reflected metadata types
// ---------------------------------------------------------------------------

/// A reflected enum value.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectEnum<'a> {
    /// The numeric wire value.
    pub number: i32,
    /// The symbolic name, if known from the descriptor.
    pub name: Option<&'a str>,
}

/// A reflected oneof group.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectOneof<'a> {
    /// The oneof name.
    pub name: String,
    /// The active field, if any.
    pub active_field: Option<ReflectField<'a>>,
}

/// A reflected field occurrence.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectField<'a> {
    /// Field id.
    pub id: u32,
    /// Field name.
    pub name: &'a str,
    /// Decoded value.
    pub value: ReflectValue<'a>,
}

/// A key/value pair in a reflected map.
pub type ReflectMapEntry<'a> = (ReflectValue<'a>, ReflectValue<'a>);

// ---------------------------------------------------------------------------
// DynamicMessage
// ---------------------------------------------------------------------------

/// A descriptor-driven dynamic message.
///
/// Holds a decoded [`RawMessage`] together with schema context so fields can
/// be inspected and mutated by name or id.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicMessage<'a> {
    raw: RawMessage,
    message: &'a ir::MessageIr,
    descriptor: &'a Descriptor,
}

impl<'a> DynamicMessage<'a> {
    /// Decodes a message from bytes using schema context.
    ///
    /// `message` is the IR representation of the expected top-level message
    /// type (typically obtained from `descriptor.find_message(name)`).
    pub fn decode(
        message: &'a ir::MessageIr,
        descriptor: &'a Descriptor,
        bytes: &[u8],
        limits: &DecoderLimits,
        policy: UnknownFieldPolicy,
    ) -> Result<Self, ReflectError> {
        let known = known_field_ids(message);
        let raw = RawMessage::decode_filtered(bytes, limits, policy, &|id| known.contains(&id))?;
        Ok(DynamicMessage {
            raw,
            message,
            descriptor,
        })
    }

    /// Encodes the message to a freshly allocated buffer.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        self.raw.encode()
    }

    /// Encodes the message in canonical/deterministic form (spec §9.10).
    ///
    /// Applies oneof last-wins reduction and key-sorted map entries before
    /// emitting the total field order.
    pub fn encode_canonical(&self) -> Result<Vec<u8>, EncodeError> {
        let mut msg = self.raw.clone();
        let groups_vec: Vec<Vec<u32>> = self
            .message
            .oneofs
            .iter()
            .map(|o| o.fields.iter().map(|f| f.id).collect::<Vec<_>>())
            .collect();
        let groups: Vec<&[u32]> = groups_vec.iter().map(|v| v.as_slice()).collect();
        msg.canonical_reduce_oneofs(&groups);
        let map_ids: Vec<u32> = self
            .message
            .fields
            .iter()
            .filter(|f| matches!(f.label, ir::FieldLabelIr::Map { .. }))
            .map(|f| f.id)
            .collect();
        msg.canonical_sort_map_entries(&map_ids);
        msg.encode_canonical()
    }

    // -----------------------------------------------------------------------
    // Field access
    // -----------------------------------------------------------------------

    /// Returns the first value of `name`, if present.
    pub fn get_field(&self, name: &str) -> Result<Option<ReflectValue<'a>>, ReflectError> {
        let field = self.resolve_field(name)?;
        self.get_value(field)
    }

    /// Returns the first value of field `id`, if present.
    pub fn get_field_id(&self, id: u32) -> Result<Option<ReflectValue<'a>>, ReflectError> {
        let field = self.resolve_field_id(id)?;
        self.get_value(field)
    }

    // -----------------------------------------------------------------------
    // Field mutation
    // -----------------------------------------------------------------------

    /// Sets (replaces) a singular field by name.
    ///
    /// For repeated fields use [`Self::add_repeated`] or [`Self::clear_repeated`].
    pub fn set_field(&mut self, name: &str, value: ReflectValue) -> Result<(), ReflectError> {
        let field = self.resolve_field(name)?;
        self.set_value(field, value)
    }

    /// Sets (replaces) a singular field by id.
    pub fn set_field_id(&mut self, id: u32, value: ReflectValue) -> Result<(), ReflectError> {
        let field = self.resolve_field_id(id)?;
        self.set_value(field, value)
    }

    /// Removes all occurrences of `name`.
    pub fn clear_field(&mut self, name: &str) -> Result<(), ReflectError> {
        let field = self.resolve_field(name)?;
        self.clear_value(field)
    }

    /// Removes all occurrences of field `id`.
    pub fn clear_field_id(&mut self, id: u32) -> Result<(), ReflectError> {
        let field = self.resolve_field_id(id)?;
        self.clear_value(field)
    }

    // -----------------------------------------------------------------------
    // Repeated field access
    // -----------------------------------------------------------------------

    /// Returns all values of repeated field `name`.
    pub fn get_repeated(
        &self,
        name: &str,
    ) -> Result<Option<Vec<ReflectValue<'a>>>, ReflectError> {
        let field = self.resolve_field(name)?;
        self.get_repeated_values(field)
    }

    /// Returns all values of repeated field `id`.
    pub fn get_repeated_id(
        &self,
        id: u32,
    ) -> Result<Option<Vec<ReflectValue<'a>>>, ReflectError> {
        let field = self.resolve_field_id(id)?;
        self.get_repeated_values(field)
    }

    /// Appends a value to repeated field `name`.
    pub fn add_repeated(&mut self, name: &str, value: ReflectValue) -> Result<(), ReflectError> {
        let field = self.resolve_field(name)?;
        self.add_repeated_value(field, value)
    }

    /// Appends a value to repeated field `id`.
    pub fn add_repeated_id(
        &mut self,
        id: u32,
        value: ReflectValue,
    ) -> Result<(), ReflectError> {
        let field = self.resolve_field_id(id)?;
        self.add_repeated_value(field, value)
    }

    /// Removes all occurrences of repeated field `name`.
    pub fn clear_repeated(&mut self, name: &str) -> Result<(), ReflectError> {
        let field = self.resolve_field(name)?;
        self.clear_value(field)
    }

    // -----------------------------------------------------------------------
    // Map access
    // -----------------------------------------------------------------------

    /// Returns all entries of map field `name`.
    ///
    /// Each entry is a `(key, value)` pair decoded from the synthetic
    /// map-entry message on the wire.
    pub fn get_map(
        &self,
        name: &str,
    ) -> Result<Option<Vec<ReflectMapEntry<'a>>>, ReflectError> {
        let field = self.resolve_field(name)?;
        self.get_map_entries(field)
    }

    /// Returns all entries of map field `id`.
    pub fn get_map_id(
        &self,
        id: u32,
    ) -> Result<Option<Vec<ReflectMapEntry<'a>>>, ReflectError> {
        let field = self.resolve_field_id(id)?;
        self.get_map_entries(field)
    }

    // -----------------------------------------------------------------------
    // Enum access
    // -----------------------------------------------------------------------

    /// Returns the enum value of field `name` with name resolution.
    pub fn get_enum(&self, name: &str) -> Result<Option<ReflectEnum<'a>>, ReflectError> {
        let field = self.resolve_field(name)?;
        self.get_enum_value(field)
    }

    /// Returns the enum value of field `id` with name resolution.
    pub fn get_enum_id(&self, id: u32) -> Result<Option<ReflectEnum<'a>>, ReflectError> {
        let field = self.resolve_field_id(id)?;
        self.get_enum_value(field)
    }

    // -----------------------------------------------------------------------
    // Oneof access
    // -----------------------------------------------------------------------

    /// Returns the active member of oneof `name`, if any.
    pub fn get_oneof(&self, name: &str) -> Result<Option<ReflectOneof<'a>>, ReflectError> {
        let oneof = self.resolve_oneof(name)?;
        self.get_oneof_value(oneof)
    }

    // -----------------------------------------------------------------------
    // Nested message access
    // -----------------------------------------------------------------------

    /// Returns the nested message value of field `name`.
    pub fn get_message(
        &self,
        name: &str,
    ) -> Result<Option<DynamicMessage<'a>>, ReflectError> {
        let field = self.resolve_field(name)?;
        self.get_nested_message(field)
    }

    /// Returns the nested message value of field `id`.
    pub fn get_message_id(
        &self,
        id: u32,
    ) -> Result<Option<DynamicMessage<'a>>, ReflectError> {
        let field = self.resolve_field_id(id)?;
        self.get_nested_message(field)
    }

    // -----------------------------------------------------------------------
    // Unknown field access
    // -----------------------------------------------------------------------

    /// Returns all unknown fields preserved on the wire.
    pub fn unknown_fields(&self) -> &[Field] {
        &self.raw.fields
    }

    /// Returns the number of unknown field occurrences.
    pub fn unknown_field_count(&self) -> usize {
        self.raw.fields.len()
    }

    // -----------------------------------------------------------------------
    // Descriptor / metadata access
    // -----------------------------------------------------------------------

    /// Returns the descriptor backing this dynamic message.
    pub fn descriptor(&self) -> &Descriptor {
        self.descriptor
    }

    /// Returns the schema fingerprint, if computed.
    pub fn fingerprint(&self) -> Option<&str> {
        self.descriptor.package.fingerprint.as_deref()
    }

    /// Returns the name of the message type this instance represents.
    pub fn message_name(&self) -> &str {
        &self.message.name
    }

    /// Returns the IR for the message type.
    pub fn message_ir(&self) -> &ir::MessageIr {
        self.message
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn resolve_field(&self, name: &str) -> Result<&'a ir::FieldIr, ReflectError> {
        self.message
            .fields
            .iter()
            .chain(self.message.oneofs.iter().flat_map(|o| &o.fields))
            .find(|f| f.name == name)
            .ok_or_else(|| ReflectError::FieldNotFound(name.to_string(), self.message.name.clone()))
    }

    fn resolve_field_id(&self, id: u32) -> Result<&'a ir::FieldIr, ReflectError> {
        self.message
            .fields
            .iter()
            .chain(self.message.oneofs.iter().flat_map(|o| &o.fields))
            .find(|f| f.id == id)
            .ok_or_else(|| ReflectError::FieldIdNotFound(id, self.message.name.clone()))
    }

    fn resolve_oneof(&self, name: &str) -> Result<&'a ir::OneofIr, ReflectError> {
        self.message
            .oneofs
            .iter()
            .find(|o| o.name == name)
            .ok_or_else(|| ReflectError::OneofNotFound(name.to_string(), self.message.name.clone()))
    }

    fn get_value(&self, field: &ir::FieldIr) -> Result<Option<ReflectValue<'a>>, ReflectError> {
        let raw = self.raw.fields.iter().find(|f| f.field_id == field.id).map(|f| &f.value);
        match raw {
            None => Ok(None),
            Some(value) => Ok(Some(interpret_value(self.descriptor, field, value)?)),
        }
    }

    fn get_repeated_values(
        &self,
        field: &ir::FieldIr,
    ) -> Result<Option<Vec<ReflectValue<'a>>>, ReflectError> {
        let values: Vec<ReflectValue<'a>> = self
            .raw
            .fields
            .iter()
            .filter(|f| f.field_id == field.id)
            .map(|f| interpret_value(self.descriptor, field, &f.value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(if values.is_empty() { None } else { Some(values) })
    }

    fn get_map_entries(
        &self,
        field: &ir::FieldIr,
    ) -> Result<Option<Vec<ReflectMapEntry<'a>>>, ReflectError> {
        let ir::FieldLabelIr::Map { key, value } = &field.label else {
            return Err(ReflectError::NotMap(field.name.clone()));
        };

        let mut entries = Vec::new();
        for f in &self.raw.fields {
            if f.field_id != field.id {
                continue;
            }
            let entry_bytes = tpt20_core::scalar::decode_bytes(&f.value)?;
            let entry = RawMessage::decode(
                entry_bytes,
                &DecoderLimits::default(),
                UnknownFieldPolicy::Preserve,
            )?;
            let mut k: Option<ReflectValue<'a>> = None;
            let mut v: Option<ReflectValue<'a>> = None;
            for ef in &entry.fields {
                match ef.field_id {
                    1 => {
                        k = Some(interpret_scalar(&key.path, &ef.value)?);
                    }
                    2 => {
                        v = Some(interpret_value(self.descriptor, &ir::FieldIr {
                            id: 0,
                            name: String::new(),
                            label: ir::FieldLabelIr::Singular(value.clone()),
                            presence: ir::Presence::Implicit,
                            annotations: vec![],
                            span: ir::SourceSpan::default(),
                        }, &ef.value)?);
                    }
                    _ => {}
                }
            }
            if let (Some(k), Some(v)) = (k, v) {
                entries.push((k, v));
            }
        }
        Ok(if entries.is_empty() { None } else { Some(entries) })
    }

    fn get_enum_value(&self, field: &ir::FieldIr) -> Result<Option<ReflectEnum<'a>>, ReflectError> {
        let raw = self.raw.fields.iter().find(|f| f.field_id == field.id).map(|f| &f.value);
        match raw {
            None => Ok(None),
            Some(value) => {
                let n = tpt20_core::scalar::decode_signed(value)? as i32;
                let name = resolve_enum_name(self.descriptor, &field_label_type_path(&field.label), n);
                Ok(Some(ReflectEnum { number: n, name }))
            }
        }
    }

    fn get_oneof_value(&self, oneof: &'a ir::OneofIr) -> Result<Option<ReflectOneof<'a>>, ReflectError> {
        let mut active: Option<(&ir::FieldIr, ReflectValue<'a>)> = None;
        for mf in &oneof.fields {
            if let Some(f) = self.raw.fields.iter().find(|f| f.field_id == mf.id) {
                let value = interpret_value(self.descriptor, mf, &f.value)?;
                active = Some((mf, value));
            }
        }
        match active {
            Some((field, value)) => Ok(Some(ReflectOneof {
                name: oneof.name.clone(),
                active_field: Some(ReflectField {
                    id: field.id,
                    name: field.name.as_str(),
                    value,
                }),
            })),
            None => Ok(None),
        }
    }

    fn get_nested_message(
        &self,
        field: &ir::FieldIr,
    ) -> Result<Option<DynamicMessage<'a>>, ReflectError> {
        let raw = self.raw.fields.iter().find(|f| f.field_id == field.id).map(|f| &f.value);
        match raw {
            None => Ok(None),
            Some(value) => {
                let bytes = tpt20_core::scalar::decode_bytes(value)?;
                let type_path = field_label_type_path(&field.label);
                let nested = find_message_by_path(self.descriptor, &type_path)
                    .ok_or_else(|| ReflectError::NotMessage(field.name.clone()))?;
                let sub = DynamicMessage::decode(nested, self.descriptor, bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve)?;
                Ok(Some(sub))
            }
        }
    }

    fn set_value(&mut self, field: &ir::FieldIr, value: ReflectValue) -> Result<(), ReflectError> {
        if matches!(field.label, ir::FieldLabelIr::Repeated(_)) {
            return Err(ReflectError::NotRepeated(field.name.clone()));
        }
        if matches!(field.label, ir::FieldLabelIr::Map { .. }) {
            return Err(ReflectError::NotMap(field.name.clone()));
        }
        let (wire_class, core_value) = reflect_value_to_core(value, &field_label_type_path(&field.label))?;
        self.raw.fields.retain(|f| f.field_id != field.id);
        self.raw.fields.push(Field::new(field.id, wire_class, core_value));
        Ok(())
    }

    fn add_repeated_value(
        &mut self,
        field: &ir::FieldIr,
        value: ReflectValue,
    ) -> Result<(), ReflectError> {
        let ir::FieldLabelIr::Repeated(type_ref) = &field.label else {
            return Err(ReflectError::NotRepeated(field.name.clone()));
        };
        let (wire_class, core_value) = reflect_value_to_core(value, &type_ref.path)?;
        self.raw.fields.push(Field::new(field.id, wire_class, core_value));
        Ok(())
    }

    fn clear_value(&mut self, field: &ir::FieldIr) -> Result<(), ReflectError> {
        self.raw.fields.retain(|f| f.field_id != field.id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn known_field_ids(message: &ir::MessageIr) -> Vec<u32> {
    let mut ids: Vec<u32> = message.fields.iter().map(|f| f.id).collect();
    for oneof in &message.oneofs {
        ids.extend(oneof.fields.iter().map(|f| f.id));
    }
    ids
}

fn field_label_type_path(label: &ir::FieldLabelIr) -> Cow<[String]> {
    match label {
        ir::FieldLabelIr::Singular(t) => Cow::Borrowed(&t.path),
        ir::FieldLabelIr::Repeated(t) => Cow::Borrowed(&t.path),
        ir::FieldLabelIr::Map { value, .. } => Cow::Borrowed(&value.path),
    }
}

fn is_scalar_path(path: &[String]) -> bool {
    path.len() == 1 && path.first().map_or(false, |p| SCALAR_NAMES.contains(&p.as_str()))
}

fn interpret_value<'a>(
    descriptor: &'a Descriptor,
    field: &ir::FieldIr,
    value: &Value,
) -> Result<ReflectValue<'a>, ReflectError> {
    let type_path = field_label_type_path(&field.label);
    let scalar = type_path.first().map(|s| s.as_str());
    match scalar {
        Some("bool") => Ok(ReflectValue::Varint(tpt20_core::scalar::decode_uint(value)?)),
        Some("int32") | Some("int64") => {
            let v = tpt20_core::scalar::decode_signed(value)?;
            Ok(ReflectValue::Varint(v as u64))
        }
        Some("uint32") | Some("uint64") => {
            let v = tpt20_core::scalar::decode_uint(value)?;
            Ok(ReflectValue::Varint(v))
        }
        Some("sint32") | Some("sint64") => {
            let v = tpt20_core::scalar::decode_sint(value)?;
            Ok(ReflectValue::Varint(v as u64))
        }
        Some("fixed32") | Some("sfixed32") => {
            let v = tpt20_core::scalar::decode_fixed32(value)?;
            Ok(ReflectValue::Fixed32(v))
        }
        Some("fixed64") | Some("sfixed64") => {
            let v = tpt20_core::scalar::decode_fixed64(value)?;
            Ok(ReflectValue::Fixed64(v))
        }
        Some("float32") => {
            let v = tpt20_core::scalar::decode_float32(value)?;
            Ok(ReflectValue::Fixed32(v.to_bits()))
        }
        Some("float64") => {
            let v = tpt20_core::scalar::decode_float64(value)?;
            Ok(ReflectValue::Fixed64(v.to_bits()))
        }
        Some("string") => {
            let s = tpt20_core::scalar::decode_string(value)?;
            Ok(ReflectValue::String(s.to_string()))
        }
        Some("bytes") => {
            let b = tpt20_core::scalar::decode_bytes(value)?;
            Ok(ReflectValue::Bytes(b.to_vec()))
        }
        _ => {
            if let Some(ei) = find_enum_by_path(descriptor, &type_path) {
                let n = tpt20_core::scalar::decode_signed(value)? as i32;
                let name = ei.values.iter().find(|v| v.number == n).map(|v| v.name.as_str());
                return Ok(ReflectValue::Enum(n, name));
            }
            if let Some(msg) = find_message_by_path(descriptor, &type_path) {
                let bytes = tpt20_core::scalar::decode_bytes(value)?;
                let sub = DynamicMessage::decode(
                    msg,
                    descriptor,
                    bytes,
                    &DecoderLimits::default(),
                    UnknownFieldPolicy::Preserve,
                )?;
                return Ok(ReflectValue::Message(Box::new(sub)));
            }
            Err(ReflectError::TypeMismatch(
                field.name.clone(),
                "unknown type",
            ))
        }
    }
}

fn interpret_scalar<'a>(path: &[String], value: &Value) -> Result<ReflectValue<'a>, ReflectError> {
    let scalar = path.first().map(|s| s.as_str());
    match scalar {
        Some("bool") => Ok(ReflectValue::Varint(tpt20_core::scalar::decode_uint(value)?)),
        Some("int32") | Some("int64") => {
            let v = tpt20_core::scalar::decode_signed(value)?;
            Ok(ReflectValue::Varint(v as u64))
        }
        Some("uint32") | Some("uint64") => {
            let v = tpt20_core::scalar::decode_uint(value)?;
            Ok(ReflectValue::Varint(v))
        }
        Some("sint32") | Some("sint64") => {
            let v = tpt20_core::scalar::decode_sint(value)?;
            Ok(ReflectValue::Varint(v as u64))
        }
        Some("fixed32") | Some("sfixed32") => {
            let v = tpt20_core::scalar::decode_fixed32(value)?;
            Ok(ReflectValue::Fixed32(v))
        }
        Some("fixed64") | Some("sfixed64") => {
            let v = tpt20_core::scalar::decode_fixed64(value)?;
            Ok(ReflectValue::Fixed64(v))
        }
        Some("float32") => {
            let v = tpt20_core::scalar::decode_float32(value)?;
            Ok(ReflectValue::Fixed32(v.to_bits()))
        }
        Some("float64") => {
            let v = tpt20_core::scalar::decode_float64(value)?;
            Ok(ReflectValue::Fixed64(v.to_bits()))
        }
        Some("string") => {
            let s = tpt20_core::scalar::decode_string(value)?;
            Ok(ReflectValue::String(s.to_string()))
        }
        Some("bytes") => {
            let b = tpt20_core::scalar::decode_bytes(value)?;
            Ok(ReflectValue::Bytes(b.to_vec()))
        }
        _ => Err(ReflectError::TypeMismatch(
            path.last().unwrap_or(&"unknown".to_string()).clone(),
            "scalar",
        )),
    }
}

fn reflect_value_to_core(
    value: ReflectValue,
    type_path: &[String],
) -> Result<(WireClass, Value), ReflectError> {
    let scalar = type_path.first().map(|s| s.as_str());
    match (scalar, value) {
        (Some("bool"), ReflectValue::Varint(v)) => Ok((WireClass::Varint, Value::Varint(v))),
        (Some("int32" | "int64" | "uint32" | "uint64" | "sint32" | "sint64"), ReflectValue::Varint(v)) => {
            Ok((WireClass::Varint, Value::Varint(v)))
        }
        (Some("fixed32" | "sfixed32" | "float32"), ReflectValue::Fixed32(v)) => {
            Ok((WireClass::Fixed32, Value::Fixed32(v)))
        }
        (Some("fixed64" | "sfixed64" | "float64"), ReflectValue::Fixed64(v)) => {
            Ok((WireClass::Fixed64, Value::Fixed64(v)))
        }
        (Some("string"), ReflectValue::String(s)) => {
            Ok((WireClass::Len, Value::Len(s.into_bytes())))
        }
        (Some("bytes"), ReflectValue::Bytes(b)) => Ok((WireClass::Len, Value::Len(b))),
        (_, ReflectValue::Enum(n, _)) => Ok((WireClass::Varint, Value::Varint(n as u64))),
        (_, ReflectValue::Message(msg)) => {
            let bytes = msg.encode()?;
            Ok((WireClass::Len, Value::Len(bytes)))
        }
        _ => Err(ReflectError::TypeMismatch(
            type_path.last().unwrap_or(&"unknown".to_string()).clone(),
            "incompatible ReflectValue variant",
        )),
    }
}

fn resolve_enum_name<'a>(descriptor: &'a Descriptor, type_path: &[String], number: i32) -> Option<&'a str> {
    find_enum_by_path(descriptor, type_path)
        .and_then(|ei| ei.values.iter().find(|v| v.number == number))
        .map(|v| v.name.as_str())
}

// ---------------------------------------------------------------------------
// Descriptor search helpers
// ---------------------------------------------------------------------------

fn find_enum_by_path<'a>(
    descriptor: &'a Descriptor,
    path: &[String],
) -> Option<&'a ir::EnumIr> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return descriptor.package.enums.iter().find(|e| e.name == path[0]);
    }
    let parent = descriptor.package.messages.iter().find(|m| m.name == path[0])?;
    find_nested_enum(parent, &path[1..])
}

fn find_nested_enum<'a>(msg: &'a ir::MessageIr, path: &[String]) -> Option<&'a ir::EnumIr> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return msg.enums.iter().find(|e| e.name == path[0]);
    }
    let child = msg.messages.iter().find(|m| m.name == path[0])?;
    find_nested_enum(child, &path[1..])
}

fn find_message_by_path<'a>(
    descriptor: &'a Descriptor,
    path: &[String],
) -> Option<&'a ir::MessageIr> {
    if path.is_empty() {
        return None;
    }
    let top = descriptor.package.messages.iter().find(|m| m.name == path[0])?;
    find_nested_message(top, &path[1..])
}

fn find_nested_message<'a>(
    msg: &'a ir::MessageIr,
    path: &[String],
) -> Option<&'a ir::MessageIr> {
    if path.is_empty() {
        return Some(msg);
    }
    let child = msg.messages.iter().find(|m| m.name == path[0])?;
    find_nested_message(child, &path[1..])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tpt20_core::DecoderLimits;
    use tpt20_ir as ir;

    fn sample_descriptor() -> Descriptor {
        let pkg = ir::PackageIr {
            name: Some("test.v1".to_string()),
            imports: vec![],
            messages: vec![
                ir::MessageIr {
                    name: "User".into(),
                    fields: vec![
                        ir::FieldIr {
                            id: 1,
                            name: "id".into(),
                            label: ir::FieldLabelIr::Singular(ir::TypeRefIr {
                                path: vec!["int64".into()],
                            }),
                            presence: ir::Presence::Implicit,
                            annotations: vec![],
                            span: ir::SourceSpan::default(),
                        },
                        ir::FieldIr {
                            id: 2,
                            name: "name".into(),
                            label: ir::FieldLabelIr::Singular(ir::TypeRefIr {
                                path: vec!["string".into()],
                            }),
                            presence: ir::Presence::Implicit,
                            annotations: vec![],
                            span: ir::SourceSpan::default(),
                        },
                        ir::FieldIr {
                            id: 3,
                            name: "email".into(),
                            label: ir::FieldLabelIr::Singular(ir::TypeRefIr {
                                path: vec!["string".into()],
                            }),
                            presence: ir::Presence::Explicit,
                            annotations: vec![],
                            span: ir::SourceSpan::default(),
                        },
                        ir::FieldIr {
                            id: 4,
                            name: "tags".into(),
                            label: ir::FieldLabelIr::Repeated(ir::TypeRefIr {
                                path: vec!["string".into()],
                            }),
                            presence: ir::Presence::Implicit,
                            annotations: vec![],
                            span: ir::SourceSpan::default(),
                        },
                        ir::FieldIr {
                            id: 5,
                            name: "meta".into(),
                            label: ir::FieldLabelIr::Map {
                                key: ir::TypeRefIr { path: vec!["string".into()] },
                                value: ir::TypeRefIr { path: vec!["string".into()] },
                            },
                            presence: ir::Presence::Implicit,
                            annotations: vec![],
                            span: ir::SourceSpan::default(),
                        },
                    ],
                    oneofs: vec![],
                    messages: vec![],
                    enums: vec![],
                    reserved: vec![],
                    annotations: vec![],
                    span: ir::SourceSpan::default(),
                },
                ir::MessageIr {
                    name: "Status".into(),
                    fields: vec![
                    ir::FieldIr {
                        id: 1,
                        name: "code".into(),
                        label: ir::FieldLabelIr::Singular(ir::TypeRefIr {
                            path: vec!["State".into()],
                        }),
                            presence: ir::Presence::Implicit,
                            annotations: vec![],
                            span: ir::SourceSpan::default(),
                        },
                    ],
                    oneofs: vec![],
                    messages: vec![],
                    enums: vec![],
                    reserved: vec![],
                    annotations: vec![],
                    span: ir::SourceSpan::default(),
                },
            ],
            enums: vec![ir::EnumIr {
                name: "State".into(),
                values: vec![
                    ir::EnumValueIr {
                        name: "Active".into(),
                        number: 0,
                        alias: false,
                    },
                    ir::EnumValueIr {
                        name: "Inactive".into(),
                        number: 1,
                        alias: false,
                    },
                ],
                open: false,
                annotations: vec![],
                span: ir::SourceSpan::default(),
            }],
            services: vec![],
            reserved: vec![],
            compat: ir::CompatMetadata::default(),
            fingerprint: None,
        };
        let mut desc = Descriptor::new(pkg);
        desc.compute_fingerprint();
        desc
    }

    fn user_message() -> ir::MessageIr {
        sample_descriptor().find_message("User").unwrap().clone()
    }

    #[test]
    fn spec_example_roundtrip() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("User").unwrap();

        // Build a raw message manually and encode it.
        let mut raw = RawMessage::new();
        raw.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
        raw.push(Field::new(2, WireClass::Len, Value::Len(b"Ada".to_vec())));
        let bytes = raw.encode().unwrap();

        // Spec §13 example: decode via descriptor, access by name, re-encode.
        let message = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        let name = message.get_field("name").unwrap();
        assert_eq!(name, Some(ReflectValue::String("Ada".to_string())));

        let id = message.get_field_id(1).unwrap();
        assert_eq!(id, Some(ReflectValue::Varint(42)));

        let reencoded = message.encode().unwrap();
        let back = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &reencoded,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();
        assert_eq!(message.raw.fields, back.raw.fields);
    }

    #[test]
    fn field_mutation() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("User").unwrap();

        let mut message = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &[],
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        message.set_field("name", ReflectValue::String("Bob".into())).unwrap();
        assert_eq!(
            message.get_field("name").unwrap(),
            Some(ReflectValue::String("Bob".to_string()))
        );

        message.set_field_id(1, ReflectValue::Varint(99)).unwrap();
        assert_eq!(message.get_field_id(1).unwrap(), Some(ReflectValue::Varint(99)));

        message.clear_field("name").unwrap();
        assert_eq!(message.get_field("name").unwrap(), None);
    }

    #[test]
    fn repeated_field_access() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("User").unwrap();

        let mut raw = RawMessage::new();
        raw.push(Field::new(4, WireClass::Len, Value::Len(b"a".to_vec())));
        raw.push(Field::new(4, WireClass::Len, Value::Len(b"b".to_vec())));
        let bytes = raw.encode().unwrap();

        let mut message = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        let tags = message.get_repeated("tags").unwrap().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], ReflectValue::String("a".to_string()));
        assert_eq!(tags[1], ReflectValue::String("b".to_string()));

        message.add_repeated("tags", ReflectValue::String("c".into())).unwrap();
        let updated = message.get_repeated("tags").unwrap().unwrap();
        assert_eq!(updated.len(), 3);
    }

    #[test]
    fn map_field_access() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("User").unwrap();

        let mut entry = RawMessage::new();
        entry.push(Field::new(1, WireClass::Len, Value::Len(b"k1".to_vec())));
        entry.push(Field::new(2, WireClass::Len, Value::Len(b"v1".to_vec())));

        let mut raw = RawMessage::new();
        raw.push(Field::new(5, WireClass::Len, Value::Len(entry.encode().unwrap())));
        let bytes = raw.encode().unwrap();

        let message = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        let map_entries = message.get_map("meta").unwrap().unwrap();
        assert_eq!(map_entries.len(), 1);
        assert_eq!(
            map_entries[0].0,
            ReflectValue::String("k1".to_string())
        );
        assert_eq!(
            map_entries[0].1,
            ReflectValue::String("v1".to_string())
        );
    }

    #[test]
    fn enum_access() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("Status").unwrap();

        let mut raw = RawMessage::new();
        raw.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
        let bytes = raw.encode().unwrap();

        let message = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        let en = message.get_enum("code").unwrap().unwrap();
        assert_eq!(en.number, 1);
        assert_eq!(en.name, Some("Inactive"));
    }

    #[test]
    fn oneof_access() {
        let descriptor = sample_descriptor();
        let mut pkg = descriptor.package.clone();
        pkg.messages[0].oneofs.push(ir::OneofIr {
            name: "contact".into(),
            fields: vec![
                ir::FieldIr {
                    id: 10,
                    name: "email".into(),
                    label: ir::FieldLabelIr::Singular(ir::TypeRefIr {
                        path: vec!["string".into()],
                    }),
                    presence: ir::Presence::Implicit,
                    annotations: vec![],
                    span: ir::SourceSpan::default(),
                },
            ],
            annotations: vec![],
            span: ir::SourceSpan::default(),
        });
        let desc = Descriptor::new(pkg);
        let msg_ir = desc.find_message("User").unwrap();

        let mut raw = RawMessage::new();
        raw.push(Field::new(10, WireClass::Len, Value::Len(b"a@b.com".to_vec())));
        let bytes = raw.encode().unwrap();

        let message = DynamicMessage::decode(
            msg_ir,
            &desc,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        let oneof = message.get_oneof("contact").unwrap().unwrap();
        assert_eq!(oneof.name, "contact");
        assert!(oneof.active_field.is_some());
        assert_eq!(oneof.active_field.unwrap().name, "email");
    }

    #[test]
    fn nested_message_access() {
        let descriptor = sample_descriptor();
        let mut pkg = descriptor.package.clone();
        pkg.messages.push(ir::MessageIr {
            name: "Address".into(),
            fields: vec![ir::FieldIr {
                id: 1,
                name: "street".into(),
                label: ir::FieldLabelIr::Singular(ir::TypeRefIr {
                    path: vec!["string".into()],
                }),
                presence: ir::Presence::Implicit,
                annotations: vec![],
                span: ir::SourceSpan::default(),
            }],
            oneofs: vec![],
            messages: vec![],
            enums: vec![],
            reserved: vec![],
            annotations: vec![],
            span: ir::SourceSpan::default(),
        });
        pkg.messages[0].fields.push(ir::FieldIr {
            id: 20,
            name: "address".into(),
            label: ir::FieldLabelIr::Singular(ir::TypeRefIr {
                path: vec!["Address".into()],
            }),
            presence: ir::Presence::Implicit,
            annotations: vec![],
            span: ir::SourceSpan::default(),
        });
        let desc = Descriptor::new(pkg);
        let msg_ir = desc.find_message("User").unwrap();

        let mut inner = RawMessage::new();
        inner.push(Field::new(1, WireClass::Len, Value::Len(b"Main St".to_vec())));
        let mut raw = RawMessage::new();
        raw.push(Field::new(20, WireClass::Len, Value::Len(inner.encode().unwrap())));
        let bytes = raw.encode().unwrap();

        let message = DynamicMessage::decode(
            msg_ir,
            &desc,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        let nested = message.get_message("address").unwrap().unwrap();
        assert_eq!(nested.message_name(), "Address");
        assert_eq!(
            nested.get_field("street").unwrap(),
            Some(ReflectValue::String("Main St".to_string()))
        );
    }

    #[test]
    fn unknown_field_access() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("User").unwrap();

        let mut raw = RawMessage::new();
        raw.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
        raw.push(Field::new(99, WireClass::Len, Value::Len(b"??".to_vec())));
        let bytes = raw.encode().unwrap();

        let message = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        assert_eq!(message.unknown_field_count(), 2);
        assert_eq!(message.unknown_fields()[1].field_id, 99);
    }

    #[test]
    fn descriptor_lookup_and_fingerprint() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("User").unwrap();
        let message = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &[],
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        assert_eq!(message.message_name(), "User");
        assert!(message.fingerprint().is_some());
        assert!(message.descriptor().find_message("User").is_some());
    }

    #[test]
    fn canonical_encode_is_deterministic() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("User").unwrap();

        let mut a = RawMessage::new();
        a.push(Field::new(2, WireClass::Len, Value::Len(b"Ada".to_vec())));
        a.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
        let bytes = a.encode().unwrap();

        let msg = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();
        let canon = msg.encode_canonical().unwrap();

        let mut b = RawMessage::new();
        b.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
        b.push(Field::new(2, WireClass::Len, Value::Len(b"Ada".to_vec())));
        let bytes2 = b.encode().unwrap();

        let msg2 = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &bytes2,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();
        assert_eq!(canon, msg2.encode_canonical().unwrap());
    }

    #[test]
    fn proxy_gateway_use_case() {
        let descriptor = sample_descriptor();
        let msg_ir = descriptor.find_message("User").unwrap();

        // Simulate receiving a message and re-emitting it (proxy/gateway).
        let mut raw = RawMessage::new();
        raw.push(Field::new(1, WireClass::Varint, Value::Varint(7)));
        raw.push(Field::new(2, WireClass::Len, Value::Len(b"proxy".to_vec())));
        let bytes = raw.encode().unwrap();

        let inbound = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();

        // Inspect fields
        assert!(inbound.get_field("id").unwrap().is_some());
        assert!(inbound.get_field("name").unwrap().is_some());

        // Re-encode for downstream
        let outbound = inbound.encode().unwrap();
        let roundtrip = DynamicMessage::decode(
            msg_ir,
            &descriptor,
            &outbound,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();
        assert_eq!(inbound.raw.fields, roundtrip.raw.fields);
    }
}

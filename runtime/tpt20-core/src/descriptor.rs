//! Lightweight message descriptor types for the dynamic message layer (spec §11.4).
//!
//! These types live in `tpt20-core` so the runtime can describe message shape
//! without depending on the compiler crate. `tpt20-descriptor` can construct
//! instances from its richer IR by populating the owned fields here.

use std::collections::HashMap;

use crate::wire::WireClass;

/// A scalar type known to the descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarKind {
    Bool,
    Int32,
    Int64,
    UInt32,
    UInt64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    SFixed32,
    SFixed64,
    Float,
    Double,
    String,
    Bytes,
    Enum { open: bool },
}

/// The kind of field a descriptor entry describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// A scalar field.
    Scalar(ScalarKind),
    /// A repeated field; `packed` is true when the wire format uses packed
    /// encoding for this field.
    Repeated { packed: bool },
    /// A map field (repeated map-entry message under the hood).
    Map,
    /// An embedded message field.
    Message,
}

/// Descriptor for a single field in a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescriptor {
    /// Numeric field id.
    pub id: u32,
    /// Field name.
    pub name: String,
    /// Wire class determined by the tag (or inferred from the scalar type).
    pub wire_class: WireClass,
    /// What kind of field this is.
    pub kind: FieldKind,
}

impl FieldDescriptor {
    /// Creates a new field descriptor.
    pub fn new(id: u32, name: impl Into<String>, wire_class: WireClass, kind: FieldKind) -> FieldDescriptor {
        FieldDescriptor {
            id,
            name: name.into(),
            wire_class,
            kind,
        }
    }

    /// Returns true if this field is a string.
    pub fn is_string(&self) -> bool {
        matches!(self.kind, FieldKind::Scalar(ScalarKind::String))
    }

    /// Returns true if this field holds raw bytes.
    pub fn is_bytes(&self) -> bool {
        matches!(self.kind, FieldKind::Scalar(ScalarKind::Bytes))
    }

    /// Returns true if this field is a length-delimited type (string, bytes, message, or packed repeated).
    pub fn is_len_delimited(&self) -> bool {
        self.wire_class == WireClass::Len
    }

    /// Returns true if this field is repeated.
    pub fn is_repeated(&self) -> bool {
        matches!(self.kind, FieldKind::Repeated { .. } | FieldKind::Map)
    }

    /// Returns true if this field is packed repeated.
    pub fn is_packed(&self) -> bool {
        matches!(self.kind, FieldKind::Repeated { packed: true })
    }
}

/// Descriptor for a oneof group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneofDescriptor {
    /// Oneof name.
    pub name: String,
    /// Member field ids.
    pub field_ids: Vec<u32>,
}

impl OneofDescriptor {
    /// Creates a new oneof descriptor.
    pub fn new(name: impl Into<String>, field_ids: Vec<u32>) -> OneofDescriptor {
        OneofDescriptor {
            name: name.into(),
            field_ids,
        }
    }
}

/// A descriptor for a message type, used by the dynamic layer to drive decode,
/// field lookup, and conversion (spec §11.4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageDescriptor {
    fields: Vec<FieldDescriptor>,
    name_to_id: HashMap<String, u32>,
    oneofs: Vec<OneofDescriptor>,
    oneof_name_to_ids: HashMap<String, Vec<u32>>,
}

impl MessageDescriptor {
    /// Creates an empty message descriptor.
    pub fn new() -> MessageDescriptor {
        MessageDescriptor::default()
    }

    /// Adds a field descriptor.
    pub fn add_field(&mut self, field: FieldDescriptor) {
        self.name_to_id.insert(field.name.clone(), field.id);
        self.fields.push(field);
    }

    /// Adds a oneof descriptor.
    pub fn add_oneof(&mut self, oneof: OneofDescriptor) {
        self.oneof_name_to_ids
            .insert(oneof.name.clone(), oneof.field_ids.clone());
        self.oneofs.push(oneof);
    }

    /// Looks up a field by id.
    pub fn field_by_id(&self, id: u32) -> Option<&FieldDescriptor> {
        self.fields.iter().find(|f| f.id == id)
    }

    /// Looks up a field by name.
    pub fn field_by_name(&self, name: &str) -> Option<&FieldDescriptor> {
        let id = *self.name_to_id.get(name)?;
        self.field_by_id(id)
    }

    /// Returns all field descriptors.
    pub fn fields(&self) -> &[FieldDescriptor] {
        &self.fields
    }

    /// Returns the number of fields.
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// Returns member field ids for a oneof by name.
    pub fn oneof_members(&self, name: &str) -> Option<&[u32]> {
        self.oneof_name_to_ids.get(name).map(|v| v.as_slice())
    }

    /// Returns all oneof descriptors.
    pub fn oneofs(&self) -> &[OneofDescriptor] {
        &self.oneofs
    }

    /// Returns true if the given field id is known to this descriptor.
    pub fn is_known(&self, id: u32) -> bool {
        self.field_by_id(id).is_some()
    }
}

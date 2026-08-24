//! Scalar type facts (spec §9.3): schema scalar name -> Rust type, wire
//! class, and packing strategy for repeated fields.

/// How a scalar packs into repeated fields (spec §9.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    /// Packed as consecutive varints.
    Varint,
    /// Packed as consecutive little-endian 32-bit words.
    Fixed32,
    /// Packed as consecutive little-endian 64-bit words.
    Fixed64,
    /// Not packable (string/bytes/messages use per-element LEN fields).
    NotPackable,
}

/// Static generation facts for one scalar type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalarInfo {
    /// Owned Rust field type.
    pub rust: &'static str,
    /// Wire class this scalar occupies (LEN only for string/bytes).
    pub class: WireClass,
    /// Packing strategy for repeated fields.
    pub pack: PackKind,
}

use crate::WireClass;

/// Returns generation facts for a schema scalar name, or `None` if the name
/// is not a known scalar (i.e. it is a message or enum reference).
pub fn scalar_info(name: &str) -> Option<ScalarInfo> {
    let info = |rust: &'static str,
                class: WireClass,
                pack: PackKind|
     -> ScalarInfo {
        ScalarInfo { rust, class, pack }
    };
    Some(match name {
        "bool" => info("bool", WireClass::Varint, PackKind::Varint),
        "int32" => info("i32", WireClass::Varint, PackKind::Varint),
        "int64" => info("i64", WireClass::Varint, PackKind::Varint),
        "uint32" => info("u32", WireClass::Varint, PackKind::Varint),
        "uint64" => info("u64", WireClass::Varint, PackKind::Varint),
        "sint32" => info("i32", WireClass::Varint, PackKind::Varint),
        "sint64" => info("i64", WireClass::Varint, PackKind::Varint),
        "fixed32" => info("u32", WireClass::Fixed32, PackKind::Fixed32),
        "sfixed32" => info("i32", WireClass::Fixed32, PackKind::Fixed32),
        "fixed64" => info("u64", WireClass::Fixed64, PackKind::Fixed64),
        "sfixed64" => info("i64", WireClass::Fixed64, PackKind::Fixed64),
        "float32" => info("f32", WireClass::Fixed32, PackKind::Fixed32),
        "float64" => info("f64", WireClass::Fixed64, PackKind::Fixed64),
        "string" => info("String", WireClass::Len, PackKind::NotPackable),
        "bytes" => info("Vec<u8>", WireClass::Len, PackKind::NotPackable),
        _ => return None,
    })
}

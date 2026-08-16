//! Neutral IR types for the tpt20 compiler (spec §8).
//!
//! The IR is the language-agnostic, in-memory representation produced after
//! parsing and consumed by semantic analysis, code generation, and descriptor
//! serialization. It deliberately mirrors the AST but adds source locations,
//! compatibility metadata, and a stable schema fingerprint.

use serde::{Deserialize, Serialize};

/// A source location (file, line, column) attached to IR nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SourceSpan {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
}

/// A whole compiled file in the neutral IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PackageIr {
    /// Package name, e.g. `"user.v1"`.
    pub name: Option<String>,
    /// Imported file paths.
    pub imports: Vec<String>,
    /// Top-level messages.
    pub messages: Vec<MessageIr>,
    /// Top-level enums.
    pub enums: Vec<EnumIr>,
    /// Top-level services.
    pub services: Vec<ServiceIr>,
    /// Top-level reserved declarations.
    pub reserved: Vec<ReservedIr>,
    /// Compatibility metadata for the package.
    pub compat: CompatMetadata,
    /// Schema fingerprint (filled after canonicalization).
    pub fingerprint: Option<String>,
}

/// A message in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageIr {
    /// Message name.
    pub name: String,
    /// Fields.
    pub fields: Vec<FieldIr>,
    /// Oneofs.
    pub oneofs: Vec<OneofIr>,
    /// Nested messages.
    pub messages: Vec<MessageIr>,
    /// Nested enums.
    pub enums: Vec<EnumIr>,
    /// Reserved declarations.
    pub reserved: Vec<ReservedIr>,
    /// Annotations.
    pub annotations: Vec<AnnotationIr>,
    /// Source location.
    pub span: SourceSpan,
}

/// Field label in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldLabelIr {
    /// Singular value.
    Singular(TypeRefIr),
    /// Repeated value.
    Repeated(TypeRefIr),
    /// Map value.
    Map { key: TypeRefIr, value: TypeRefIr },
}

/// A field in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldIr {
    /// Numeric field id (wire contract).
    pub id: u32,
    /// Field name.
    pub name: String,
    /// Label.
    pub label: FieldLabelIr,
    /// Presence semantics.
    pub presence: Presence,
    /// Annotations.
    pub annotations: Vec<AnnotationIr>,
    /// Source location.
    pub span: SourceSpan,
}

/// Presence semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Presence {
    /// Absence equals default.
    Implicit,
    /// Absence distinguishable from default.
    Explicit,
}

/// A oneof in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OneofIr {
    /// Oneof name.
    pub name: String,
    /// Member fields.
    pub fields: Vec<FieldIr>,
    /// Annotations.
    pub annotations: Vec<AnnotationIr>,
    /// Source location.
    pub span: SourceSpan,
}

/// An enum in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumIr {
    /// Enum name.
    pub name: String,
    /// Values.
    pub values: Vec<EnumValueIr>,
    /// Whether open.
    pub open: bool,
    /// Annotations.
    pub annotations: Vec<AnnotationIr>,
    /// Source location.
    pub span: SourceSpan,
}

/// An enum value in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumValueIr {
    /// Value name.
    pub name: String,
    /// Stable number.
    pub number: i32,
    /// Alias flag.
    pub alias: bool,
}

/// A service in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceIr {
    /// Service name.
    pub name: String,
    /// Methods.
    pub methods: Vec<MethodIr>,
    /// Annotations.
    pub annotations: Vec<AnnotationIr>,
    /// Source location.
    pub span: SourceSpan,
}

/// A method in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodIr {
    /// Method name.
    pub name: String,
    /// Request type.
    pub request: TypeRefIr,
    /// Request streaming.
    pub request_streaming: bool,
    /// Response type.
    pub response: TypeRefIr,
    /// Response streaming.
    pub response_streaming: bool,
    /// Annotations.
    pub annotations: Vec<AnnotationIr>,
}

/// A type reference in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRefIr {
    /// Dotted path segments.
    pub path: Vec<String>,
}

impl TypeRefIr {
    /// Returns the bare type name (last segment).
    pub fn name(&self) -> &str {
        self.path.last().map(|s| s.as_str()).unwrap_or("")
    }
}

/// An annotation in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationIr {
    /// Annotation name.
    pub name: String,
    /// Arguments.
    pub args: Vec<AnnotationArgIr>,
}

/// An annotation argument in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationArgIr {
    /// Identifier argument.
    Ident(String),
    /// String argument.
    String(String),
    /// Integer argument.
    Int(i64),
    /// Boolean argument.
    Bool(bool),
}

/// A reserved declaration in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservedIr {
    /// Reserved ids / ranges.
    pub ids: Vec<ReservedIdIr>,
    /// Reserved names.
    pub names: Vec<String>,
}

/// A reserved id entry in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservedIdIr {
    /// Single id.
    Single(u32),
    /// Inclusive range.
    Range(u32, u32),
}

/// Compatibility metadata attached to a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompatMetadata {
    /// Dominant compatibility policy for the package.
    pub policy: String,
    /// Recorded schema versions.
    pub versions: Vec<String>,
    /// Deprecation dates, keyed by symbol.
    pub deprecations: Vec<String>,
}

/// Computes a stable, deterministic fingerprint string from a canonicalized
/// IR package.
///
/// The fingerprint is derived from the JSON serialization of the IR after
/// clearing the `fingerprint` field, so semantically-identical packages yield
/// identical fingerprints (spec §8, §9 canonical mode).
pub fn fingerprint(pkg: &PackageIr) -> String {
    let mut canonical = pkg.clone();
    canonical.fingerprint = None;
    let json = serde_json::to_string(&canonical).unwrap_or_default();
    // FNV-1a 64-bit hash rendered as hex; stable and dependency-free.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in json.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

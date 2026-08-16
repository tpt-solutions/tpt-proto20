//! AST data structures for the tpt20 schema language (spec §6).
//!
//! These types are produced by the [`crate::parser`] and consumed by the
//! semantic-analysis and codegen passes. Source locations are attached where
//! useful so the diagnostics engine (Phase 2) can render helpful messages.

/// A complete parsed `.tpt` file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct File {
    /// The `package` declaration, if any (e.g. `"user.v1"`).
    pub package: Option<String>,
    /// `import` paths.
    pub imports: Vec<String>,
    /// Top-level message declarations.
    pub messages: Vec<Message>,
    /// Top-level enum declarations.
    pub enums: Vec<Enum>,
    /// Top-level service declarations.
    pub services: Vec<Service>,
    /// Top-level reserved declarations.
    pub reserved: Vec<Reserved>,
}

/// A message declaration (possibly nested).
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Message name.
    pub name: String,
    /// Field declarations.
    pub fields: Vec<Field>,
    /// Oneof declarations.
    pub oneofs: Vec<Oneof>,
    /// Nested message declarations.
    pub messages: Vec<Message>,
    /// Nested enum declarations.
    pub enums: Vec<Enum>,
    /// Reserved declarations (ids and/or names).
    pub reserved: Vec<Reserved>,
    /// Annotations applied to the message.
    pub annotations: Vec<Annotation>,
}

/// A field declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Numeric field id — part of the wire contract.
    pub id: u32,
    /// Field name.
    pub name: String,
    /// Field label (singular / repeated / map).
    pub label: FieldLabel,
    /// Presence semantics (implicit or explicit `?`).
    pub presence: Presence,
    /// Annotations applied to the field.
    pub annotations: Vec<Annotation>,
}

/// Field label / cardinality.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldLabel {
    /// A single optional-or-default value (implicit or explicit presence).
    Singular(TypeRef),
    /// A repeated list `repeated T`.
    Repeated(TypeRef),
    /// A map `map<K, V>`.
    Map {
        /// Key type (scalar or string).
        key: TypeRef,
        /// Value type.
        value: TypeRef,
    },
}

/// Presence semantics (spec §6 implicit vs explicit presence).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Absence equals the default value (implicit presence).
    Implicit,
    /// Absence is distinguishable from default (`?` syntax).
    Explicit,
}

/// A oneof declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Oneof {
    /// Oneof name.
    pub name: String,
    /// Mutually exclusive member fields.
    pub fields: Vec<Field>,
    /// Annotations applied to the oneof.
    pub annotations: Vec<Annotation>,
}

/// An enum declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    /// Enum name.
    pub name: String,
    /// Enum values.
    pub values: Vec<EnumValue>,
    /// Whether the enum is open (unknown values tolerated) or closed.
    pub open: bool,
    /// Annotations applied to the enum.
    pub annotations: Vec<Annotation>,
}

/// An enum value.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumValue {
    /// Value name.
    pub name: String,
    /// Stable numeric value.
    pub number: i32,
    /// Whether this is an explicit alias of another value.
    pub alias: bool,
}

/// A service declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    /// Service name.
    pub name: String,
    /// Method declarations.
    pub methods: Vec<Method>,
    /// Annotations applied to the service.
    pub annotations: Vec<Annotation>,
}

/// A service method declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    /// Method name.
    pub name: String,
    /// Request type.
    pub request: TypeRef,
    /// Whether the request is client-streamed.
    pub request_streaming: bool,
    /// Response type.
    pub response: TypeRef,
    /// Whether the response is server-streamed.
    pub response_streaming: bool,
    /// Annotations applied to the method.
    pub annotations: Vec<Annotation>,
}

/// A type reference (scalar or qualified message/enum name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    /// Dotted path segments, e.g. `["user", "v1", "User"]`.
    pub path: Vec<String>,
}

impl TypeRef {
    /// Builds a type reference from a single segment.
    pub fn scalar(name: &str) -> TypeRef {
        TypeRef {
            path: vec![name.to_string()],
        }
    }

    /// Returns the last path segment (the bare type name).
    pub fn name(&self) -> &str {
        self.path.last().map(|s| s.as_str()).unwrap_or("")
    }
}

/// An annotation `@name(args)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// Annotation name.
    pub name: String,
    /// Argument expressions.
    pub args: Vec<AnnotationArg>,
}

/// An annotation argument.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationArg {
    /// A bare identifier argument.
    Ident(String),
    /// A string literal argument.
    String(String),
    /// An integer literal argument.
    Int(i64),
    /// A boolean literal argument.
    Bool(bool),
}

/// A reserved declaration (reserved ids and/or names).
#[derive(Debug, Clone, PartialEq)]
pub struct Reserved {
    /// Reserved numeric ids / id ranges.
    pub ids: Vec<ReservedId>,
    /// Reserved names.
    pub names: Vec<String>,
}

/// A reserved id entry: a single id or an inclusive range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservedId {
    /// A single reserved id.
    Single(u32),
    /// An inclusive range `lo..=hi`.
    Range(u32, u32),
}

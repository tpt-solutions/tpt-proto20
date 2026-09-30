//! Schema helpers: scalar table, type resolution, errors.

use thiserror::Error;
use tpt20_core::WireClass;
use tpt20_ir as ir;

/// Errors from printing or parsing text format.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TextError {
    /// The requested message type does not exist in the descriptor.
    #[error("unknown message `{0}`")]
    UnknownMessage(String),
    /// A field type could not be resolved against the descriptor.
    #[error("unresolved type `{0}`")]
    UnresolvedType(String),
    /// Text syntax error.
    #[error("syntax error at {line}:{col}: {msg}")]
    Syntax {
        /// 1-based line.
        line: usize,
        /// 1-based column.
        col: usize,
        /// What went wrong.
        msg: String,
    },
    /// A field name is not part of the message.
    #[error("unknown field `{field}` in message `{message}` at {line}:{col}")]
    UnknownField {
        /// Message name.
        message: String,
        /// Field name in the text.
        field: String,
        /// 1-based line.
        line: usize,
        /// 1-based column.
        col: usize,
    },
    /// A value does not have the type the field requires.
    #[error("field `{field}`: expected {expected}")]
    TypeMismatch {
        /// Field name.
        field: String,
        /// Expected kind of value.
        expected: &'static str,
    },
    /// A number does not fit the field type.
    #[error("field `{0}`: value out of range")]
    OutOfRange(String),
    /// A singular field was set more than once.
    #[error("field `{0}` set more than once")]
    DuplicateField(String),
    /// Two members of one oneof were set.
    #[error("oneof `{0}` set more than once")]
    OneofConflict(String),
    /// An enum name or number is not valid for the enum.
    #[error("field `{field}`: invalid enum value `{value}`")]
    InvalidEnum {
        /// Field name.
        field: String,
        /// The offending text.
        value: String,
    },
    /// A string field did not contain valid UTF-8.
    #[error("field `{0}`: invalid UTF-8")]
    InvalidUtf8(String),
    /// The wire value does not match the schema type of the field.
    #[error("field `{0}`: wire value does not match schema type")]
    WireMismatch(String),
    /// A map entry lacked its key or value.
    #[error("map field `{0}`: entry needs both `key` and `value`")]
    MalformedMapEntry(String),
    /// Nesting is deeper than the configured limit.
    #[error("nesting depth limit exceeded")]
    DepthExceeded,
    /// A size or count limit was exceeded.
    #[error("limit exceeded: {0}")]
    LimitExceeded(&'static str),
    /// Decoding wire bytes failed.
    #[error("decode error: {0}")]
    Decode(String),
    /// Encoding to wire bytes failed.
    #[error("encode error: {0}")]
    Encode(String),
}

/// Scalar schema types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scalar {
    Bool,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Sfixed32,
    Fixed64,
    Sfixed64,
    Float32,
    Float64,
    String,
    Bytes,
}

impl Scalar {
    pub(crate) fn from_name(name: &str) -> Option<Scalar> {
        Some(match name {
            "bool" => Scalar::Bool,
            "int32" => Scalar::Int32,
            "int64" => Scalar::Int64,
            "uint32" => Scalar::Uint32,
            "uint64" => Scalar::Uint64,
            "sint32" => Scalar::Sint32,
            "sint64" => Scalar::Sint64,
            "fixed32" => Scalar::Fixed32,
            "sfixed32" => Scalar::Sfixed32,
            "fixed64" => Scalar::Fixed64,
            "sfixed64" => Scalar::Sfixed64,
            "float32" => Scalar::Float32,
            "float64" => Scalar::Float64,
            "string" => Scalar::String,
            "bytes" => Scalar::Bytes,
            _ => return None,
        })
    }

    pub(crate) fn wire_class(self) -> WireClass {
        match self {
            Scalar::Bool
            | Scalar::Int32
            | Scalar::Int64
            | Scalar::Uint32
            | Scalar::Uint64
            | Scalar::Sint32
            | Scalar::Sint64 => WireClass::Varint,
            Scalar::Fixed32 | Scalar::Sfixed32 | Scalar::Float32 => WireClass::Fixed32,
            Scalar::Fixed64 | Scalar::Sfixed64 | Scalar::Float64 => WireClass::Fixed64,
            Scalar::String | Scalar::Bytes => WireClass::Len,
        }
    }

    pub(crate) fn packable(self) -> bool {
        self.wire_class() != WireClass::Len
    }
}

/// A resolved field type.
#[derive(Debug, Clone)]
pub(crate) enum Kind<'a> {
    Scalar(Scalar),
    Enum(&'a ir::EnumIr),
    /// Message type; the chain runs outermost → the message itself and is
    /// the scope used to resolve that message's own field types.
    Message(Vec<&'a ir::MessageIr>),
}

/// Resolves type paths against a package.
pub(crate) struct Resolver<'a> {
    pub(crate) package: &'a ir::PackageIr,
}

impl<'a> Resolver<'a> {
    /// Finds a message by dotted name from the package root.
    pub(crate) fn locate(&self, name: &str) -> Result<Vec<&'a ir::MessageIr>, TextError> {
        let path: Vec<String> = name.split('.').map(str::to_string).collect();
        match descend(
            &self.package.messages,
            &self.package.enums,
            &path,
            Vec::new(),
        ) {
            Some(Kind::Message(chain)) => Ok(chain),
            _ => Err(TextError::UnknownMessage(name.to_string())),
        }
    }

    /// Resolves `path` as seen from inside the message at the end of `scope`.
    pub(crate) fn resolve(
        &self,
        scope: &[&'a ir::MessageIr],
        path: &[String],
    ) -> Result<Kind<'a>, TextError> {
        if path.len() == 1 {
            if let Some(s) = Scalar::from_name(&path[0]) {
                return Ok(Kind::Scalar(s));
            }
        }
        for depth in (0..=scope.len()).rev() {
            let (msgs, enums) = if depth == 0 {
                (&self.package.messages, &self.package.enums)
            } else {
                (&scope[depth - 1].messages, &scope[depth - 1].enums)
            };
            if let Some(k) = descend(msgs, enums, path, scope[..depth].to_vec()) {
                return Ok(k);
            }
        }
        Err(TextError::UnresolvedType(path.join(".")))
    }
}

fn descend<'a>(
    msgs: &'a [ir::MessageIr],
    enums: &'a [ir::EnumIr],
    path: &[String],
    mut chain: Vec<&'a ir::MessageIr>,
) -> Option<Kind<'a>> {
    let (first, rest) = path.split_first()?;
    if rest.is_empty() {
        if let Some(m) = msgs.iter().find(|m| &m.name == first) {
            chain.push(m);
            return Some(Kind::Message(chain));
        }
        return enums.iter().find(|e| &e.name == first).map(Kind::Enum);
    }
    let m = msgs.iter().find(|m| &m.name == first)?;
    chain.push(m);
    descend(&m.messages, &m.enums, rest, chain)
}

/// A field with its oneof membership.
pub(crate) struct FieldRef<'a> {
    pub(crate) field: &'a ir::FieldIr,
    /// Index into `MessageIr::oneofs` when the field is a oneof member.
    pub(crate) oneof: Option<usize>,
}

/// All fields of a message (regular + oneof members), sorted by field id.
pub(crate) fn all_fields(msg: &ir::MessageIr) -> Vec<FieldRef<'_>> {
    let mut out: Vec<FieldRef<'_>> = msg
        .fields
        .iter()
        .map(|field| FieldRef { field, oneof: None })
        .collect();
    for (i, o) in msg.oneofs.iter().enumerate() {
        out.extend(o.fields.iter().map(|field| FieldRef {
            field,
            oneof: Some(i),
        }));
    }
    out.sort_by_key(|f| f.field.id);
    out
}

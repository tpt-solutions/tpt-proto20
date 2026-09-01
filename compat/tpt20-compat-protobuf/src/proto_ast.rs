//! Intermediate AST produced by the `.proto` lexer and parser (spec §10.1).
//!
//! This is separate from the tpt20 schema AST; lowering converts this into
//! `tpt20_ir::PackageIr`.

use crate::error::WireError;

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// A token produced by the lexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Token {
        Token { kind, span }
    }
}

/// The kind of token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Syntax,
    Import,
    Public,
    Weak,
    Option,
    Package,
    Message,
    Enum,
    Oneof,
    Map,
    Reserved,
    Extend,
    Service,
    Rpc,
    Returns,
    Stream,
    Optional,
    Repeated,
    Required,
    Default,
    Max,
    Deprecated,
    Packed,
    Float,
    Double,
    Int32,
    Int64,
    UInt32,
    UInt64,
    SInt32,
    SInt64,
    Fixed32,
    Fixed64,
    SFixed32,
    SFixed64,
    Bool,
    String,
    Bytes,
    True,
    False,

    // Symbols
    Semi,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LAngle,
    RAngle,
    Comma,
    Eq,
    Dot,
    Lt,
    Gt,
    To,

    // Literals
    Ident(String),
    StringLit(String),
    IntLit(i64),
    FloatLit(String),

    Eof,
}

// ---------------------------------------------------------------------------
// Span
// ---------------------------------------------------------------------------

/// A source location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

impl Default for Span {
    fn default() -> Span {
        Span { line: 1, column: 1 }
    }
}

// ---------------------------------------------------------------------------
// Proto AST
// ---------------------------------------------------------------------------

/// A complete parsed `.proto` file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProtoFile {
    pub syntax: Option<String>,
    pub package: Option<String>,
    pub imports: Vec<Import>,
    pub options: Vec<OptionDecl>,
    pub messages: Vec<Message>,
    pub enums: Vec<Enum>,
    pub services: Vec<Service>,
    pub extensions: Vec<Extend>,
    pub reserved: Vec<Reserved>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub public: bool,
    pub weak: bool,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionDecl {
    pub name: String,
    pub value: OptionValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OptionValue {
    Ident(String),
    StringLit(String),
    Int(i64),
    Float(String),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub name: String,
    pub fields: Vec<Field>,
    pub oneofs: Vec<Oneof>,
    pub messages: Vec<Message>,
    pub enums: Vec<Enum>,
    pub extensions: Vec<Extend>,
    pub reserved: Vec<Reserved>,
    pub options: Vec<OptionDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub label: FieldLabel,
    pub field_type: ProtoType,
    pub name: String,
    pub number: u32,
    pub options: Vec<OptionDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldLabel {
    Singular,
    Repeated,
    Optional,
    Required,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProtoType {
    Double,
    Float,
    Int32,
    Int64,
    UInt32,
    UInt64,
    SInt32,
    SInt64,
    Fixed32,
    Fixed64,
    SFixed32,
    SFixed64,
    Bool,
    String,
    Bytes,
    Message { name: Vec<String> },
    Enum { name: Vec<String> },
    Map { key: Box<ProtoType>, value: Box<ProtoType> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Oneof {
    pub name: String,
    pub fields: Vec<Field>,
    pub options: Vec<OptionDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    pub name: String,
    pub values: Vec<EnumValue>,
    pub options: Vec<OptionDecl>,
    pub allow_alias: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumValue {
    pub name: String,
    pub number: i32,
    pub options: Vec<OptionDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub name: String,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    pub name: String,
    pub request_type: Vec<String>,
    pub request_streaming: bool,
    pub response_type: Vec<String>,
    pub response_streaming: bool,
    pub options: Vec<OptionDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extend {
    pub message_type: Vec<String>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reserved {
    pub ids: Vec<ReservedId>,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReservedId {
    Single(u32),
    Range(u32, u32),
}

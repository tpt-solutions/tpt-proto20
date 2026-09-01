//! Error types for the protobuf compatibility adapter.

use thiserror::Error;

/// Errors produced while parsing or lowering `.proto` files.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtoError {
    /// A lexing error occurred.
    #[error("lex error at line {line}, column {column}: {message}")]
    Lex {
        /// 1-based line number.
        line: usize,
        /// 1-based column number.
        column: usize,
        /// Human-readable error message.
        message: String,
    },

    /// An unexpected token was found during parsing.
    #[error("unexpected token {found:?} at line {line}, column {column}, expected {expected}")]
    UnexpectedToken {
        /// The token that was found.
        found: String,
        /// 1-based line number.
        line: usize,
        /// 1-based column number.
        column: usize,
        /// What was expected.
        expected: &'static str,
    },

    /// End of input was reached unexpectedly.
    #[error("unexpected end of input")]
    UnexpectedEof,

    /// An unsupported protobuf construct was encountered.
    #[error("unsupported protobuf construct: {0}")]
    Unsupported(&'static str),

    /// A reserved field id or name was duplicated.
    #[error("duplicate reserved declaration")]
    DuplicateReserved,

    /// An extension field id overlapped with a declared field.
    #[error("extension field id {0} conflicts with declared field")]
    ExtensionConflict(u32),

    /// A package or type name was empty.
    #[error("empty identifier")]
    EmptyIdentifier,
}

/// Errors produced by the protobuf wire adapter.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WireError {
    /// A varint overflow occurred.
    #[error("varint overflow")]
    VarintOverflow,

    /// The input was truncated.
    #[error("truncated input")]
    Truncated,

    /// A length-delimited field declared an impossible length.
    #[error("invalid length")]
    InvalidLength,

    /// A string field contained invalid UTF-8.
    #[error("invalid UTF-8 in string field")]
    InvalidUtf8,

    /// An unsupported wire type was encountered in protobuf input.
    #[error("unsupported protobuf wire type {0}")]
    UnsupportedWireType(u8),

    /// A group wire type was encountered (not supported by tpt20-core).
    #[error("group wire types are not supported by tpt20")]
    GroupWireType,

    /// An internal invariant was violated.
    #[error("internal error: {0}")]
    Internal(&'static str),
}

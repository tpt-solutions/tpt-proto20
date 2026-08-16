//! Core error type for the `tpt20` native wire format.
//!
//! Decoding is intended to be safe against untrusted input; every error
//! variant corresponds to a malformed or limit-violating payload.

use thiserror::Error;

/// Errors that can occur while encoding or decoding tpt20 messages.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// An unexpected end-of-input was reached.
    #[error("unexpected end of input (truncated message)")]
    Truncated,

    /// A varint was longer than 10 bytes (64-bit overflow).
    #[error("varint too long (would overflow 64 bits)")]
    VarintOverflow,

    /// A length-delimited field declared a negative or impossible length.
    #[error("invalid length-delimited length")]
    InvalidLength,

    /// A string field contained invalid UTF-8.
    #[error("string field contained invalid UTF-8")]
    InvalidUtf8,

    /// A length-delimited payload exceeded the configured size limit.
    #[error("payload exceeded configured byte limit ({limit} bytes)")]
    LimitExceeded {
        /// The limit that was violated.
        limit: usize,
    },

    /// A message nested deeper than the configured maximum depth.
    #[error("maximum nesting depth exceeded")]
    DepthExceeded,

    /// More fields than the configured maximum field count were present.
    #[error("maximum field count exceeded")]
    FieldCountExceeded,

    /// A repeated field contained more entries than the configured maximum.
    #[error("maximum repeated entries exceeded")]
    RepeatedEntriesExceeded,

    /// A map contained more entries than the configured maximum.
    #[error("maximum map entries exceeded")]
    MapEntriesExceeded,

    /// A scalar value (e.g. fixed32/64) was encoded with a wrong-length payload.
    #[error("malformed fixed-width scalar")]
    MalformedScalar,

    /// Unknown fields were present and the policy is `Fail`.
    #[error("unknown field encountered (fail policy)")]
    UnknownFieldForbidden,

    /// An internal invariant was violated; this indicates a bug in the codec.
    #[error("internal error: {0}")]
    Internal(&'static str),
}

/// Errors that can occur while encoding tpt20 messages.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EncodeError {
    /// An internal invariant was violated; this indicates a bug in the codec.
    #[error("internal error: {0}")]
    Internal(&'static str),
}

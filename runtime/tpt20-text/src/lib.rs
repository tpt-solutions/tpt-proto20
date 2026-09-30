//! `tpt20-text`: human-readable text format for tpt20 messages (spec §14.3).
//!
//! ```text
//! id: 42
//! name: "Ada"
//! email: "ada@example.com"
//! tags: "a"
//! tags: "b"
//! home {
//!   street: "1 Way"
//! }
//! attrs {
//!   key: "k"
//!   value: "v"
//! }
//! ```
//!
//! The format is schema-driven: [`TextFormat`] uses a [`Descriptor`] to map
//! between field names and ids, so it prints and parses the raw wire model
//! ([`RawMessage`]) for any message in the descriptor.
//!
//! - **Repeated fields** are one `name: value` line per element (the parser
//!   also accepts `name: [a, b]`). Packed wire encodings are unpacked when
//!   printing.
//! - **Maps** are repeated `name { key: … value: … }` entries, printed sorted
//!   by key.
//! - **Oneofs** print only the member that wins on the wire (last one set);
//!   the parser rejects setting two members of one oneof.
//! - **Nested messages** use `name { … }` with two-space indentation.
//! - **Enums** print by name (or number if the value is unknown) and parse by
//!   name or number (numbers that are not declared are rejected for closed
//!   enums).
//! - **Deterministic output**: fields are printed in field-id order, repeated
//!   elements in wire order, map entries sorted by key. Unknown fields are
//!   not printed.
//! - `#` starts a comment; `,` and `;` are accepted as optional separators.
//!
//! Parsing is bounded by [`TextFormat::max_depth`] and
//! [`TextFormat::max_text_bytes`]; malformed input returns [`TextError`] and
//! never panics.

mod parse;
mod print;
mod schema;

use tpt20_core::{DecoderLimits, RawMessage};
use tpt20_descriptor::Descriptor;

pub use schema::TextError;

/// Text printer/parser bound to a schema.
#[derive(Debug, Clone)]
pub struct TextFormat<'a> {
    descriptor: &'a Descriptor,
    /// Maximum message nesting depth accepted when printing or parsing.
    pub max_depth: usize,
    /// Maximum size of text accepted by the parser, in bytes.
    pub max_text_bytes: usize,
    limits: DecoderLimits,
}

impl<'a> TextFormat<'a> {
    /// Creates a text format for `descriptor` with default limits.
    pub fn new(descriptor: &'a Descriptor) -> Self {
        let limits = DecoderLimits::default();
        TextFormat {
            descriptor,
            max_depth: limits.max_depth,
            max_text_bytes: limits.max_message_bytes,
            limits,
        }
    }

    /// Prints `message` (a message name such as `User` or `Outer.Child`) as text.
    pub fn print(&self, message: &str, raw: &RawMessage) -> Result<String, TextError> {
        print::print(self, message, raw)
    }

    /// Encodes `bytes` (native wire format) of `message` as text.
    pub fn print_bytes(&self, message: &str, bytes: &[u8]) -> Result<String, TextError> {
        let raw = RawMessage::decode(
            bytes,
            &self.limits,
            tpt20_core::UnknownFieldPolicy::Preserve,
        )
        .map_err(|e| TextError::Decode(e.to_string()))?;
        self.print(message, &raw)
    }

    /// Parses text into the raw wire model of `message`.
    pub fn parse(&self, message: &str, text: &str) -> Result<RawMessage, TextError> {
        parse::parse(self, message, text)
    }

    /// Parses text and encodes it straight to the native wire format.
    pub fn parse_to_bytes(&self, message: &str, text: &str) -> Result<Vec<u8>, TextError> {
        self.parse(message, text)?
            .encode()
            .map_err(|e| TextError::Encode(e.to_string()))
    }

    pub(crate) fn descriptor(&self) -> &'a Descriptor {
        self.descriptor
    }

    pub(crate) fn limits(&self) -> &DecoderLimits {
        &self.limits
    }
}

#[cfg(test)]
mod tests;

//! `tpt20-compat-protobuf`: Protobuf compatibility adapter for tpt-proto20 (spec §10).
//!
//! Provides:
//! - `.proto` schema import (proto2, proto3, Editions) → `tpt20_ir::PackageIr`
//! - Protobuf wire format encode/decode adapters
//!
//! ## Proto schema import
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let src = std::fs::read_to_string("user.proto")?;
//! let tokens = tpt20_compat_protobuf::lexer::lex(&src)?;
//! let proto_ast = tpt20_compat_protobuf::parser::parse(tokens)?;
//! let ir = tpt20_compat_protobuf::lower(&proto_ast)?;
//! # Ok(()) }
//! ```
//!
//! ## Protobuf wire adapter
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let bytes = b"\x08\x96\x01"; // field 1, varint 150
//! let msg = tpt20_compat_protobuf::wire::decode_protobuf(bytes)?;
//! let encoded = tpt20_compat_protobuf::wire::encode_protobuf(&msg)?;
//! # Ok(()) }
//! ```

pub mod error;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod proto_ast;
pub mod wire;

pub use error::{ProtoError, WireError};
pub use lexer::lex as lex_proto;
pub use lower::lower;
pub use parser::parse as parse_proto;
pub use wire::{decode_protobuf, encode_protobuf, decode_protobuf_with, encode_protobuf_with};

//! Native conformance suite (spec §22.1).
//!
//! Validates tpt20's native implementation against the specification across
//! schema parsing, semantic analysis, wire encoding/decoding, canonical
//! encoding, JSON/text mapping, reflection, dynamic messages, RPC behavior,
//! streaming, deadlines, cancellation, and security limits.

pub mod schema_parsing;
pub mod semantic_analysis;
pub mod wire_encoding;
pub mod wire_decoding;
pub mod canonical_encoding;
pub mod json_mapping;
pub mod text_mapping;
pub mod reflection;
pub mod dynamic_message;
pub mod rpc_behavior;
pub mod streaming_behavior;
pub mod deadline_behavior;
pub mod cancellation_behavior;
pub mod security_limits;

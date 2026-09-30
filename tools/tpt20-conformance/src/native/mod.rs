//! Native conformance suite (spec §22.1).
//!
//! Validates tpt20's native implementation against the specification across
//! schema parsing, semantic analysis, wire encoding/decoding, canonical
//! encoding, JSON/text mapping, reflection, dynamic messages, RPC behavior,
//! streaming, deadlines, cancellation, and security limits.

pub mod cancellation_behavior;
pub mod canonical_encoding;
pub mod deadline_behavior;
pub mod dynamic_message;
pub mod json_mapping;
pub mod reflection;
pub mod rpc_behavior;
pub mod schema_parsing;
pub mod security_limits;
pub mod semantic_analysis;
pub mod streaming_behavior;
pub mod text_mapping;
pub mod wire_decoding;
pub mod wire_encoding;

//! Compatibility conformance suite (spec §22.2).
//!
//! Validates tpt20's compatibility adapters against protobuf and gRPC:
//! schema import, wire format roundtrips, status/metadata mapping, and
//! streaming semantics.

pub mod protobuf_schema_import;
pub mod protobuf_binary_decoding;
pub mod protobuf_binary_encoding;
pub mod grpc_rpc_behavior;
pub mod status_mapping;
pub mod metadata_mapping;
pub mod streaming_semantics;

//! `tpt20-conformance`: conformance and testing suite for tpt20 (spec §22).
//!
//! This crate is the single source of truth for all conformance validation:
//!
//! - **Native conformance suite** — validates schema parsing, semantic analysis,
//!   wire encoding/decoding, canonical encoding, JSON/text mapping, reflection,
//!   dynamic messages, RPC behavior, streaming, deadlines, cancellation, and
//!   security limits against the tpt20 specification.
//! - **Compatibility conformance suite** — validates protobuf schema import,
//!   protobuf wire adapter, gRPC behavior, status/metadata/streaming mapping.
//! - **Fuzz targets** — standalone binaries for binary decoder, JSON decoder,
//!   text parser, schema parser, descriptor decoder, dynamic message decoder,
//!   RPC framing, and metadata parsing.
//! - **Property-based roundtrip tests** — `encode -> decode -> equal` and
//!   `decode -> encode -> decode -> equal`.
//! - **Rust ↔ Rust interoperability baseline** — validates that two independent
//!   Rust implementations can exchange messages.

pub mod native;
pub mod compat;
pub mod roundtrip;
pub mod interop;

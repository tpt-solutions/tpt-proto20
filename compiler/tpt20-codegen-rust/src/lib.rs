//! `tpt20-codegen-rust`: Rust code generator for tpt20 schemas (spec §12).
//!
//! Consumes the neutral IR ([`tpt20_ir::PackageIr`]) and emits a single,
//! self-contained Rust module containing:
//!
//! - owned message structs with `encode` / `decode` / `decode_with_limits` /
//!   `encode_canonical` / `to_raw`;
//! - borrowed view types (`XView<'a>`) with `decode_borrowed` for zero-copy
//!   access to string/bytes payloads;
//! - generated enums respecting open/closed unknown-value semantics;
//! - oneofs as mutually exclusive Rust enums;
//! - JSON conversion methods (`to_json` / `from_json`) per spec §14.2;
//! - opt-in validated builders.
//!
//! Generated code depends on `tpt20-core` and `tpt20-json`.
//!
//! Service code generation is deferred until the RPC runtime types exist
//! (spec §16, todo Phase 10); it will extend this module.

pub mod emit;
pub mod expr;
pub mod model;
pub mod naming;
pub mod scalars;

use tpt20_ir as ir;

/// Wire classes mirrored from spec §9 for codegen reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireClass {
    /// Variable-length integer.
    Varint,
    /// 32-bit fixed little-endian value.
    Fixed32,
    /// 64-bit fixed little-endian value.
    Fixed64,
    /// Length-delimited payload.
    Len,
}

/// Options controlling generation output.
#[derive(Debug, Clone)]
pub struct CodegenOptions {
    /// Emit builders with annotation validation (spec §12.3).
    pub builders: bool,
    /// Crate name to reference for `tpt20-core` in generated code.
    pub core_crate: String,
    /// Crate name to reference for `tpt20-json` in generated code.
    pub json_crate: String,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        CodegenOptions {
            builders: false,
            core_crate: "tpt20_core".to_string(),
            json_crate: "tpt20_json".to_string(),
        }
    }
}

/// Generates a single Rust source module for the package.
pub fn generate_module(package: &ir::PackageIr, opts: &CodegenOptions) -> String {
    let e = emit::Emitter::new(package, opts);
    e.generate()
}

/// Suggested output file stem for a package (`user.v1` -> `user_v1`).
pub fn output_file_stem(package: &ir::PackageIr) -> String {
    naming::package_file_stem(package.name.as_deref())
}

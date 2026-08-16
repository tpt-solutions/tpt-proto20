//! `tpt20-compiler`: semantic analysis, compatibility checks, IR/descriptor
//! generation, and schema-history manifest support for the tpt20 compiler
//! (spec §7, §8, §20).
//!
//! Pipeline:
//! `.tpt` → lexer → parser → [`tpt20_language`] AST → [`semantic`] analysis →
//! [`ast_to_ir`] lowering → [`tpt20_ir`] IR → [`tpt20_descriptor`] descriptor →
//! fingerprint.

pub mod ast_to_ir;
pub mod compat;
pub mod diagnostics;
pub mod manifest;
pub mod pipeline;
pub mod semantic;

pub use ast_to_ir::lower;
pub use compat::{diff, render_report, ChangeClass, CompatChange};
pub use diagnostics::{render_all, Diagnostic, Severity};
pub use manifest::SchemaHistoryManifest;
pub use pipeline::{check, compile, diff_sources, CompileOutput};
pub use semantic::{analyze, analyze_with_imports, AnnotationRegistry, KNOWN_SCALARS};

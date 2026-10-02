//! Multi-language code generation driven by the neutral IR (spec §3.4, §22.5).
//!
//! Rust is the reference implementation (`tpt20-codegen-rust`). This crate
//! adds backends for **Go**, **Java** and **Python**. Each backend turns a
//! [`tpt20_ir::PackageIr`] into self-contained source files: the generated
//! message code plus a *minimal runtime* (wire format, limits, and the RPC
//! client/server where the language supports it) emitted next to it, so the
//! output needs no extra dependency.
//!
//! All backends share [`model`], a resolved, language-neutral view of the
//! schema, and implement [`Backend`]:
//!
//! ```
//! use tpt20_codegen_backends::{backend, BackendOptions};
//! let ir = tpt20_ir::PackageIr {
//!     name: Some("demo.v1".into()),
//!     ..Default::default()
//! };
//! let files = backend("python").unwrap().generate(&ir, &BackendOptions::default()).unwrap();
//! assert!(files.iter().any(|f| f.path.ends_with(".py")));
//! ```
//!
//! What the generated code implements (see `docs/polyglot.md`): binary encode
//! and decode with the same decoder limits as the Rust runtime, unknown-field
//! preservation, packed/unpacked repeated fields, maps, oneofs, open/closed
//! enums and explicit presence. JSON and text formats are Rust-only.

pub mod go;
pub mod java;
pub mod model;
pub mod naming;
pub mod python;

use tpt20_ir as ir;

/// One generated file, with a path relative to the output directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// Relative output path (forward slashes).
    pub path: String,
    /// File contents.
    pub contents: String,
}

/// Options shared by all backends.
#[derive(Debug, Clone, Default)]
pub struct BackendOptions {
    /// Overrides the language package/module name (default: derived from the
    /// schema package).
    pub package_name: Option<String>,
    /// Also generate service clients/servers where the backend supports them.
    pub services: bool,
}

/// Why generation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    /// A type reference could not be resolved.
    #[error("unresolved type `{0}`")]
    UnresolvedType(String),
    /// The schema uses something the backend cannot express.
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// A code generation backend.
pub trait Backend {
    /// Backend name (`"go"`, `"java"`, `"python"`).
    fn name(&self) -> &'static str;

    /// Generates all files for `package`.
    fn generate(
        &self,
        package: &ir::PackageIr,
        options: &BackendOptions,
    ) -> Result<Vec<GeneratedFile>, BackendError>;
}

/// Looks a backend up by name.
pub fn backend(name: &str) -> Option<Box<dyn Backend>> {
    match name {
        "go" => Some(Box::new(go::GoBackend)),
        "java" => Some(Box::new(java::JavaBackend)),
        "python" | "py" => Some(Box::new(python::PythonBackend)),
        _ => None,
    }
}

/// Names of all registered backends.
pub const BACKENDS: &[&str] = &["go", "java", "python"];

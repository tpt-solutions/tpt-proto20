//! Full compiler pipeline wiring (spec §7.1).
//!
//! `.tpt` source → lexer → parser → AST → semantic analysis → IR generation →
//! descriptor generation → fingerprint. Diagnostics are collected from every
//! stage. The compatibility detector and schema history manifest are exposed
//! for tooling (Phases 16 / 20).

use crate::ast_to_ir::lower;
use crate::compat;
use crate::diagnostics::Diagnostic;
use crate::diagnostics::Severity;
use crate::semantic;
use tpt20_descriptor::Descriptor;
use tpt20_ir as ir;
use tpt20_language::parse;

/// The result of a successful compilation.
#[derive(Debug, Clone)]
pub struct CompileOutput {
    /// Neutral IR package.
    pub ir: ir::PackageIr,
    /// Runtime descriptor wrapping the IR.
    pub descriptor: Descriptor,
    /// Stable schema fingerprint.
    pub fingerprint: String,
    /// Diagnostics produced during analysis (may include warnings).
    pub diagnostics: Vec<Diagnostic>,
}

/// Converts a parse error into a diagnostic.
fn parse_error_to_diagnostic(e: tpt20_language::ParseError, file: &str) -> Diagnostic {
    use tpt20_language::ParseError;
    match e {
        ParseError::RequiredNotAllowed(span) => Diagnostic::error(
            "E0011",
            "the `required` keyword is not part of the tpt20 language",
        )
        .in_file(file)
        .at(span.line, span.column)
        .with_help("remove `required`; use implicit or explicit (`?`) presence instead"),
        ParseError::UnexpectedToken { found, at } => {
            Diagnostic::error("E0012", format!("unexpected token: {found}"))
                .in_file(file)
                .at(at.line, at.column)
        }
        ParseError::UnexpectedEof => {
            Diagnostic::error("E0013", "unexpected end of input").in_file(file)
        }
        ParseError::MissingFieldId(at) => {
            Diagnostic::error("E0014", "expected a numeric field id before `:`")
                .in_file(file)
                .at(at.line, at.column)
        }
        ParseError::ExpectedType(at) => Diagnostic::error("E0015", "expected a type name")
            .in_file(file)
            .at(at.line, at.column),
        ParseError::ExpectedNumber(at) => Diagnostic::error("E0016", "expected a numeric literal")
            .in_file(file)
            .at(at.line, at.column),
        ParseError::Lex(unexpected) => {
            Diagnostic::error("E0017", format!("lexing error: {unexpected:?}")).in_file(file)
        }
    }
}

/// Compiles `.tpt` source into IR, a descriptor, and a fingerprint.
///
/// Returns `Err` if parsing or semantic analysis produced any errors. A
/// `file` name may be supplied for diagnostics; it defaults to `"schema.tpt"`.
pub fn compile(src: &str, file: Option<&str>) -> Result<CompileOutput, Vec<Diagnostic>> {
    let fname = file.unwrap_or("schema.tpt").to_string();

    let ast = parse(src).map_err(|e| vec![parse_error_to_diagnostic(e, &fname)])?;

    let diagnostics = semantic::analyze(&ast);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(diagnostics);
    }

    let mut ir_pkg = lower(&ast);
    let mut descriptor = Descriptor::new(ir_pkg.clone());
    let fingerprint = descriptor.compute_fingerprint();
    ir_pkg = descriptor.package.clone();

    Ok(CompileOutput {
        ir: ir_pkg,
        descriptor,
        fingerprint,
        diagnostics,
    })
}

/// Runs semantic analysis and returns diagnostics without producing a
/// descriptor (used by `tpt20 check` / `tpt20 lint`).
pub fn check(src: &str, file: Option<&str>) -> Vec<Diagnostic> {
    let fname = file.unwrap_or("schema.tpt").to_string();
    match parse(src) {
        Ok(ast) => semantic::analyze(&ast),
        Err(e) => vec![parse_error_to_diagnostic(e, &fname)],
    }
}

/// Compares two `.tpt` sources for compatibility, returning the change report.
pub fn diff_sources(
    old_src: &str,
    new_src: &str,
) -> Result<Vec<compat::CompatChange>, Vec<Diagnostic>> {
    let old = compile(old_src, Some("old.tpt"))?;
    let new = compile(new_src, Some("new.tpt"))?;
    Ok(compat::diff(&old.ir, &new.ir))
}

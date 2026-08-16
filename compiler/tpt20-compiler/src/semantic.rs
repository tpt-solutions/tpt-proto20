//! Semantic analysis pass for the tpt20 compiler (spec §7, §20).
//!
//! Produces [`Diagnostic`]s for a parsed schema: duplicate field/enum/name
//! detection, unresolved imports, oneof validity, map key/value validity,
//! field-ID/reserved collisions, and annotation argument validation.

use crate::diagnostics::Diagnostic;
use tpt20_language::ast;

/// Scalar type names recognized by the language (spec §9.3).
pub const KNOWN_SCALARS: &[&str] = &[
    "bool", "int32", "int64", "uint32", "uint64", "sint32", "sint64", "fixed32", "fixed64",
    "sfixed32", "sfixed64", "float32", "float64", "string", "bytes",
];

fn is_scalar(name: &str) -> bool {
    KNOWN_SCALARS.contains(&name)
}

/// A registry of known/standardized annotations and custom annotations
/// (spec §6.9, §25.1). Unknown annotations are reported as warnings unless a
/// custom annotation has been registered.
#[derive(Debug, Clone, Default)]
pub struct AnnotationRegistry {
    /// Names of known (built-in or registered) annotations.
    known: std::collections::HashSet<String>,
}

impl AnnotationRegistry {
    /// Creates a registry seeded with the standardized built-in annotations.
    pub fn builtins() -> AnnotationRegistry {
        let mut r = AnnotationRegistry::default();
        for name in [
            "max_len",
            "min_len",
            "range",
            "pattern",
            "default",
            "deprecated",
        ] {
            r.known.insert(name.to_string());
        }
        r
    }

    /// Registers a custom annotation name, suppressing the "unknown annotation"
    /// warning for it.
    pub fn register(&mut self, name: &str) {
        self.known.insert(name.to_string());
    }

    /// Returns whether the annotation name is recognized.
    pub fn is_known(&self, name: &str) -> bool {
        self.known.contains(name)
    }
}

fn id_in_reserved(id: u32, reserved: &[ast::Reserved]) -> bool {
    for r in reserved {
        for rid in &r.ids {
            match rid {
                ast::ReservedId::Single(n) => {
                    if *n == id {
                        return true;
                    }
                }
                ast::ReservedId::Range(lo, hi) => {
                    if id >= *lo && id <= *hi {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn all_field_ids(msg: &ast::Message) -> Vec<(u32, String, tpt20_language::lexer::Span)> {
    let mut out: Vec<(u32, String, tpt20_language::lexer::Span)> = msg
        .fields
        .iter()
        .map(|f| (f.id, f.name.clone(), f.span))
        .collect();
    for oneof in &msg.oneofs {
        for f in &oneof.fields {
            out.push((f.id, format!("{}.{}", oneof.name, f.name), f.span));
        }
    }
    out
}

fn analyze_message(
    msg: &ast::Message,
    file: &str,
    reg: &AnnotationRegistry,
    declared: &std::collections::HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    // Reserved-id reuse: a live field may not occupy a reserved id.
    for f in &msg.fields {
        if id_in_reserved(f.id, &msg.reserved) {
            diags.push(
                Diagnostic::error(
                    "E0006",
                    format!("field `{}` reuses reserved field id {}", f.name, f.id),
                )
                .in_file(file)
                .at(f.span.line, f.span.column)
                .with_help("choose a non-reserved field id, or remove the reservation"),
            );
        }
    }

    // Duplicate field ids within the message scope (including oneof members).
    let ids = all_field_ids(msg);
    let mut seen: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for (id, name, span) in &ids {
        if let Some(prev) = seen.get(id) {
            diags.push(
                Diagnostic::error(
                    "E0001",
                    format!("duplicate field id {id} in message `{}`", msg.name),
                )
                .in_file(file)
                .at(span.line, span.column)
                .with_help(format!("field id {id} is already used by `{prev}`")),
            );
        } else {
            seen.insert(*id, name.clone());
        }
    }

    // Duplicate declaration names within the message scope.
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for f in &msg.fields {
        if !names.insert(f.name.clone()) {
            diags.push(
                Diagnostic::error(
                    "E0003",
                    format!("duplicate name `{}` in message `{}`", f.name, msg.name),
                )
                .in_file(file)
                .at(f.span.line, f.span.column),
            );
        }
    }
    for o in &msg.oneofs {
        if !names.insert(o.name.clone()) {
            diags.push(
                Diagnostic::error(
                    "E0003",
                    format!("duplicate name `{}` in message `{}`", o.name, msg.name),
                )
                .in_file(file)
                .at(o.span.line, o.span.column),
            );
        }
        // Oneof members must be singular (no repeated / map) and unique within
        // their own set.
        let mut member_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for mf in &o.fields {
            if !matches!(mf.label, ast::FieldLabel::Singular(_)) {
                diags.push(
                    Diagnostic::error(
                        "E0005",
                        format!(
                            "oneof `{}` member `{}` must be singular (not repeated or map)",
                            o.name, mf.name
                        ),
                    )
                    .in_file(file)
                    .at(mf.span.line, mf.span.column),
                );
            }
            if !member_names.insert(mf.name.clone()) {
                diags.push(
                    Diagnostic::error(
                        "E0003",
                        format!("duplicate oneof member `{}` in oneof `{}`", mf.name, o.name),
                    )
                    .in_file(file)
                    .at(mf.span.line, mf.span.column),
                );
            }
            validate_type(&mf.label, file, mf.span, declared, diags);
        }
    }

    // Map key/value validity for regular fields.
    for f in &msg.fields {
        validate_type(&f.label, file, f.span, declared, diags);
        validate_annotations(&f.annotations, file, f.span, reg, diags);
    }
    for o in &msg.oneofs {
        validate_annotations(&o.annotations, file, o.span, reg, diags);
    }
    validate_annotations(&msg.annotations, file, msg.span, reg, diags);

    // Recurse into nested declarations.
    for m in &msg.messages {
        analyze_message(m, file, reg, declared, diags);
    }
    for e in &msg.enums {
        analyze_enum(e, file, reg, diags);
    }
}

fn validate_type(
    label: &ast::FieldLabel,
    file: &str,
    span: tpt20_language::lexer::Span,
    declared: &std::collections::HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    match label {
        ast::FieldLabel::Singular(t) | ast::FieldLabel::Repeated(t) => {
            validate_typeref(t, file, span, declared, diags);
        }
        ast::FieldLabel::Map { key, value } => {
            // Key must be a scalar or string (spec §6.5).
            let key_name = key.name();
            if !(is_scalar(key_name) && key_name != "bytes") {
                diags.push(
                    Diagnostic::error(
                        "E0007",
                        format!(
                            "map key type `{key_name}` is not allowed (must be scalar or string)"
                        ),
                    )
                    .in_file(file)
                    .at(span.line, span.column)
                    .with_help("use a scalar (e.g. int32) or string key type"),
                );
            }
            validate_typeref(value, file, span, declared, diags);
        }
    }
}

fn validate_typeref(
    t: &ast::TypeRef,
    file: &str,
    span: tpt20_language::lexer::Span,
    declared: &std::collections::HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    let name = t.name();
    // Scalar references are fine; a single-segment reference that matches a
    // declared message/enum name is fine; qualified (multi-segment) references
    // are accepted syntactically and resolved at link time. Anything else that
    // looks like a scalar is flagged.
    if t.path.len() == 1 && !is_scalar(name) && !declared.contains(name) {
        diags.push(
            Diagnostic::error("E0010", format!("unknown scalar type `{name}`"))
                .in_file(file)
                .at(span.line, span.column)
                .with_help("use one of the scalar types or a declared message/enum name"),
        );
    }
}

fn analyze_enum(en: &ast::Enum, file: &str, reg: &AnnotationRegistry, diags: &mut Vec<Diagnostic>) {
    // Duplicate non-alias enum value numbers.
    let mut seen: std::collections::HashMap<i32, String> = std::collections::HashMap::new();
    for v in &en.values {
        if v.alias {
            continue;
        }
        if let Some(prev) = seen.get(&v.number) {
            diags.push(
                Diagnostic::error(
                    "E0002",
                    format!("duplicate enum value {} in enum `{}`", v.number, en.name),
                )
                .in_file(file)
                .at(v.span.line, v.span.column)
                .with_help(format!("value {} is already used by `{prev}`", v.number)),
            );
        } else {
            seen.insert(v.number, v.name.clone());
        }
    }
    validate_annotations(&en.annotations, file, en.span, reg, diags);
}

fn validate_annotations(
    anns: &[ast::Annotation],
    file: &str,
    span: tpt20_language::lexer::Span,
    reg: &AnnotationRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    for a in anns {
        if !reg.is_known(&a.name) {
            diags.push(
                Diagnostic::warning("E0009", format!("unknown annotation `@{}`", a.name))
                    .in_file(file)
                    .at(span.line, span.column)
                    .with_help("register the annotation or use a standardized one"),
            );
            continue;
        }
        // Validate argument shapes for standardized annotations.
        let (ok, kind) = match a.name.as_str() {
            "max_len" | "min_len" | "range" => (
                a.args
                    .iter()
                    .all(|arg| matches!(arg, ast::AnnotationArg::Int(_))),
                "integer argument(s)",
            ),
            "pattern" | "default" => (
                a.args
                    .iter()
                    .any(|arg| matches!(arg, ast::AnnotationArg::String(_))),
                "a string argument",
            ),
            _ => (true, ""),
        };
        if !ok {
            diags.push(
                Diagnostic::error(
                    "E0009",
                    format!("annotation `@{}` expects {}", a.name, kind),
                )
                .in_file(file)
                .at(span.line, span.column),
            );
        }
    }
}

/// Collects the set of all declared message and enum type names in a file
/// (including nested declarations) so single-segment references can be
/// distinguished from unknown scalar types.
fn collect_declared_types(file: &ast::File) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    fn walk_message(m: &ast::Message, set: &mut std::collections::HashSet<String>) {
        set.insert(m.name.clone());
        for n in &m.messages {
            walk_message(n, set);
        }
        for e in &m.enums {
            set.insert(e.name.clone());
        }
    }
    for m in &file.messages {
        walk_message(m, &mut set);
    }
    for e in &file.enums {
        set.insert(e.name.clone());
    }
    set
}

/// Runs semantic analysis over a parsed file, returning all diagnostics
/// (errors and warnings).
pub fn analyze(file: &ast::File) -> Vec<Diagnostic> {
    analyze_with_imports(file, &[])
}

/// Runs semantic analysis, treating the provided paths as resolvable imports.
/// Any `import` not present in `known_files` yields an unresolved-import error.
pub fn analyze_with_imports(file: &ast::File, known_files: &[String]) -> Vec<Diagnostic> {
    let reg = AnnotationRegistry::builtins();
    let declared = collect_declared_types(file);
    let file_name = file
        .package
        .as_deref()
        .map(|p| format!("{p}.tpt"))
        .unwrap_or_else(|| "schema.tpt".to_string());
    let mut diags = Vec::new();

    // Unresolved imports.
    for imp in &file.imports {
        if !known_files.iter().any(|f| f == imp) {
            diags.push(
                Diagnostic::error("E0004", format!("unresolved import `{imp}`"))
                    .in_file(&file_name)
                    .at(file.span.line, file.span.column)
                    .with_help("ensure the imported file is part of the compilation unit"),
            );
        }
    }

    // Top-level duplicate declaration names.
    let mut top_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for m in &file.messages {
        if !top_names.insert(m.name.clone()) {
            diags.push(
                Diagnostic::error("E0003", format!("duplicate message `{}`", m.name))
                    .in_file(&file_name)
                    .at(m.span.line, m.span.column),
            );
        }
        analyze_message(m, &file_name, &reg, &declared, &mut diags);
    }
    for e in &file.enums {
        if !top_names.insert(e.name.clone()) {
            diags.push(
                Diagnostic::error("E0003", format!("duplicate enum `{}`", e.name))
                    .in_file(&file_name)
                    .at(e.span.line, e.span.column),
            );
        }
        analyze_enum(e, &file_name, &reg, &mut diags);
    }
    for s in &file.services {
        if !top_names.insert(s.name.clone()) {
            diags.push(
                Diagnostic::error("E0003", format!("duplicate service `{}`", s.name))
                    .in_file(&file_name)
                    .at(s.span.line, s.span.column),
            );
        }
        validate_annotations(&s.annotations, &file_name, s.span, &reg, &mut diags);
    }

    diags
}

//! Lowering pass: `tpt20-language` AST → neutral [`tpt20_ir`] IR (spec §8).
//!
//! Source spans from the parsed AST are carried through so the diagnostics
//! engine and descriptor lookups remain accurate.

use tpt20_ir as ir;
use tpt20_language::ast;

fn map_span(s: tpt20_language::lexer::Span) -> ir::SourceSpan {
    ir::SourceSpan {
        line: s.line,
        column: s.column,
    }
}

fn map_type(t: &ast::TypeRef) -> ir::TypeRefIr {
    ir::TypeRefIr {
        path: t.path.clone(),
    }
}

fn map_annotation(a: &ast::Annotation) -> ir::AnnotationIr {
    ir::AnnotationIr {
        name: a.name.clone(),
        args: a
            .args
            .iter()
            .map(|arg| match arg {
                ast::AnnotationArg::Ident(s) => ir::AnnotationArgIr::Ident(s.clone()),
                ast::AnnotationArg::String(s) => ir::AnnotationArgIr::String(s.clone()),
                ast::AnnotationArg::Int(n) => ir::AnnotationArgIr::Int(*n),
                ast::AnnotationArg::Bool(b) => ir::AnnotationArgIr::Bool(*b),
            })
            .collect(),
    }
}

fn map_annotations(a: &[ast::Annotation]) -> Vec<ir::AnnotationIr> {
    a.iter().map(map_annotation).collect()
}

fn map_label(l: &ast::FieldLabel) -> ir::FieldLabelIr {
    match l {
        ast::FieldLabel::Singular(t) => ir::FieldLabelIr::Singular(map_type(t)),
        ast::FieldLabel::Repeated(t) => ir::FieldLabelIr::Repeated(map_type(t)),
        ast::FieldLabel::Map { key, value } => ir::FieldLabelIr::Map {
            key: map_type(key),
            value: map_type(value),
        },
    }
}

fn map_field(f: &ast::Field) -> ir::FieldIr {
    ir::FieldIr {
        id: f.id,
        name: f.name.clone(),
        label: map_label(&f.label),
        presence: match f.presence {
            ast::Presence::Implicit => ir::Presence::Implicit,
            ast::Presence::Explicit => ir::Presence::Explicit,
        },
        annotations: map_annotations(&f.annotations),
        span: map_span(f.span),
    }
}

fn map_oneof(o: &ast::Oneof) -> ir::OneofIr {
    ir::OneofIr {
        name: o.name.clone(),
        fields: o.fields.iter().map(map_field).collect(),
        annotations: map_annotations(&o.annotations),
        span: map_span(o.span),
    }
}

fn map_message(m: &ast::Message) -> ir::MessageIr {
    ir::MessageIr {
        name: m.name.clone(),
        fields: m.fields.iter().map(map_field).collect(),
        oneofs: m.oneofs.iter().map(map_oneof).collect(),
        messages: m.messages.iter().map(map_message).collect(),
        enums: m.enums.iter().map(map_enum).collect(),
        reserved: map_reserved(&m.reserved),
        annotations: map_annotations(&m.annotations),
        span: map_span(m.span),
    }
}

fn map_enum(e: &ast::Enum) -> ir::EnumIr {
    ir::EnumIr {
        name: e.name.clone(),
        values: e
            .values
            .iter()
            .map(|v| ir::EnumValueIr {
                name: v.name.clone(),
                number: v.number,
                alias: v.alias,
                // Note: EnumValueIr has no span field; location is preserved on
                // the parent enum / via the descriptor's source metadata.
            })
            .collect(),
        open: e.open,
        annotations: map_annotations(&e.annotations),
        span: map_span(e.span),
    }
}

fn map_service(s: &ast::Service) -> ir::ServiceIr {
    ir::ServiceIr {
        name: s.name.clone(),
        methods: s
            .methods
            .iter()
            .map(|m| ir::MethodIr {
                name: m.name.clone(),
                request: map_type(&m.request),
                request_streaming: m.request_streaming,
                response: map_type(&m.response),
                response_streaming: m.response_streaming,
                annotations: map_annotations(&m.annotations),
            })
            .collect(),
        annotations: map_annotations(&s.annotations),
        span: map_span(s.span),
    }
}

fn map_reserved(r: &[ast::Reserved]) -> Vec<ir::ReservedIr> {
    r.iter()
        .map(|r| ir::ReservedIr {
            ids: r
                .ids
                .iter()
                .map(|id| match id {
                    ast::ReservedId::Single(n) => ir::ReservedIdIr::Single(*n),
                    ast::ReservedId::Range(lo, hi) => ir::ReservedIdIr::Range(*lo, *hi),
                })
                .collect(),
            names: r.names.clone(),
        })
        .collect()
}

/// Lowers a parsed AST file into a neutral IR package.
pub fn lower(file: &ast::File) -> ir::PackageIr {
    ir::PackageIr {
        name: file.package.clone(),
        imports: file.imports.clone(),
        messages: file.messages.iter().map(map_message).collect(),
        enums: file.enums.iter().map(map_enum).collect(),
        services: file.services.iter().map(map_service).collect(),
        reserved: map_reserved(&file.reserved),
        compat: ir::CompatMetadata::default(),
        fingerprint: None,
    }
}

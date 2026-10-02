//! Lowers the `.proto` AST into `tpt20_ir::PackageIr` (spec §10.1).
//!
//! Handles proto2, proto3 and Editions (2023/2024) semantics:
//!
//! * field presence: proto3 implicit, proto2 explicit, editions via
//!   `features.field_presence` (file, message or field level);
//! * enum openness: proto3/editions open, proto2 closed, editions via
//!   `features.enum_type`;
//! * `map<K, V>` becomes a map field, `double`/`float` become
//!   `float64`/`float32`;
//! * type references are resolved with protobuf scoping rules against the
//!   types declared in the file (references to other files stay as written);
//! * `extend` blocks whose extendee is declared in the same file are merged
//!   into the extendee as ordinary fields (same field number, same wire
//!   form); extensions of external messages (such as `google.protobuf.*Options`
//!   custom options) are dropped and reported.

use std::collections::{HashMap, HashSet};

use tpt20_ir as ir;

use crate::proto_ast::*;
use crate::ProtoError;

/// What lowering could not represent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LowerReport {
    /// Extension fields whose extendee is not declared in this file, as
    /// `extendee.field` (custom options and cross-file extensions).
    pub dropped_extensions: Vec<String>,
}

/// Lowers a parsed `.proto` file into a `PackageIr`.
pub fn lower(proto: ProtoFile) -> Result<ir::PackageIr, ProtoError> {
    lower_with_report(proto).map(|(pkg, _)| pkg)
}

/// Like [`lower`], also reporting what was dropped.
pub fn lower_with_report(proto: ProtoFile) -> Result<(ir::PackageIr, LowerReport), ProtoError> {
    let dialect = Dialect::of(&proto)?;
    let package = proto.package.clone().unwrap_or_default();
    let mut declared: HashMap<String, Declared> = HashMap::new();
    for m in &proto.messages {
        collect_message(m, "", &mut declared);
    }
    for e in &proto.enums {
        declared.insert(e.name.clone(), Declared::Enum);
    }

    let file_features = dialect.file_features(&proto.options)?;
    let cx = Cx {
        package: &package,
        declared: &declared,
    };

    let mut pkg = ir::PackageIr {
        name: proto.package.clone(),
        imports: proto.imports.iter().map(|i| i.path.clone()).collect(),
        ..Default::default()
    };
    for msg in &proto.messages {
        pkg.messages
            .push(lower_message(&cx, msg, "", file_features)?);
    }
    for en in &proto.enums {
        pkg.enums.push(lower_enum(en, file_features)?);
    }
    for svc in &proto.services {
        pkg.services.push(lower_service(&cx, svc));
    }
    pkg.reserved = proto.reserved.iter().cloned().map(lower_reserved).collect();

    // Extensions: file level first, then those nested in messages.
    let mut report = LowerReport::default();
    for ext in &proto.extensions {
        merge_extend(
            &cx,
            &mut pkg,
            &proto.messages,
            ext,
            "",
            file_features,
            &mut report,
        )?;
    }
    fn nested(
        cx: &Cx<'_>,
        pkg: &mut ir::PackageIr,
        roots: &[Message],
        msgs: &[Message],
        prefix: &str,
        features: Features,
        report: &mut LowerReport,
    ) -> Result<(), ProtoError> {
        for m in msgs {
            let full = join(prefix, &m.name);
            let mf = features.with_options(&m.options)?;
            for ext in &m.extensions {
                merge_extend(cx, pkg, roots, ext, &full, mf, report)?;
            }
            nested(cx, pkg, roots, &m.messages, &full, mf, report)?;
        }
        Ok(())
    }
    nested(
        &cx,
        &mut pkg,
        &proto.messages,
        &proto.messages,
        "",
        file_features,
        &mut report,
    )?;

    Ok((pkg, report))
}

// ---------------------------------------------------------------------------
// Dialects and features
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Proto2,
    Proto3,
    Edition,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Presence {
    Explicit,
    Implicit,
    LegacyRequired,
}

/// The subset of edition features that changes how schemas lower.
#[derive(Clone, Copy)]
struct Features {
    presence: Presence,
    open_enums: bool,
}

impl Dialect {
    fn of(proto: &ProtoFile) -> Result<Dialect, ProtoError> {
        match (proto.syntax.as_deref(), proto.edition.as_deref()) {
            (Some(_), Some(_)) => Err(ProtoError::Unsupported("both syntax and edition given")),
            (_, Some("2023" | "2024")) => Ok(Dialect::Edition),
            (_, Some(_)) => Err(ProtoError::Unsupported("unknown protobuf edition")),
            (Some("proto3"), None) => Ok(Dialect::Proto3),
            (Some("proto2") | None, None) => Ok(Dialect::Proto2),
            (Some(_), None) => Err(ProtoError::Unsupported("unknown protobuf syntax")),
        }
    }

    fn file_features(self, options: &[OptionDecl]) -> Result<Features, ProtoError> {
        let base = match self {
            Dialect::Proto2 => Features {
                presence: Presence::Explicit,
                open_enums: false,
            },
            Dialect::Proto3 => Features {
                presence: Presence::Implicit,
                open_enums: true,
            },
            Dialect::Edition => Features {
                presence: Presence::Explicit,
                open_enums: true,
            },
        };
        if self == Dialect::Edition {
            base.with_options(options)
        } else {
            Ok(base)
        }
    }
}

impl Features {
    /// Applies `features.*` options (only meaningful in editions; in other
    /// dialects they are ignored, as protoc rejects them anyway).
    fn with_options(mut self, options: &[OptionDecl]) -> Result<Features, ProtoError> {
        for o in options {
            let OptionValue::Ident(v) = &o.value else {
                continue;
            };
            match o.name.as_str() {
                "features.field_presence" => {
                    self.presence = match v.as_str() {
                        "EXPLICIT" => Presence::Explicit,
                        "IMPLICIT" => Presence::Implicit,
                        "LEGACY_REQUIRED" => Presence::LegacyRequired,
                        _ => return Err(ProtoError::Unsupported("unknown field_presence")),
                    }
                }
                "features.enum_type" => {
                    self.open_enums = match v.as_str() {
                        "OPEN" => true,
                        "CLOSED" => false,
                        _ => return Err(ProtoError::Unsupported("unknown enum_type")),
                    }
                }
                "features.message_encoding" if v == "DELIMITED" => {
                    return Err(ProtoError::Unsupported(
                        "delimited (group) message encoding",
                    ));
                }
                _ => {}
            }
        }
        Ok(self)
    }
}

// ---------------------------------------------------------------------------
// Type resolution
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Declared {
    Message,
    Enum,
}

struct Cx<'a> {
    package: &'a str,
    declared: &'a HashMap<String, Declared>,
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

fn collect_message(m: &Message, prefix: &str, out: &mut HashMap<String, Declared>) {
    let full = join(prefix, &m.name);
    out.insert(full.clone(), Declared::Message);
    for e in &m.enums {
        out.insert(join(&full, &e.name), Declared::Enum);
    }
    for n in &m.messages {
        collect_message(n, &full, out);
    }
}

impl Cx<'_> {
    /// Package-relative name for a type written as `written` inside `scope`
    /// (a package-relative message name, empty at file level).
    fn resolve(&self, scope: &str, written: &str) -> String {
        // Absolute: `.pkg.v1.Foo`.
        if let Some(abs) = written.strip_prefix('.') {
            return self.strip_package(abs);
        }
        // Package-qualified without a leading dot.
        let stripped = self.strip_package(written);
        if stripped != written && self.declared.contains_key(&stripped) {
            return stripped;
        }
        // Protobuf scoping: the first component is searched from the
        // innermost scope outwards; the rest must then resolve inside it.
        let first = written.split('.').next().unwrap_or(written);
        let mut scope = scope.to_string();
        loop {
            let head = join(&scope, first);
            if self.declared.contains_key(&head) {
                let full = join(&scope, written);
                return if self.declared.contains_key(&full) {
                    full
                } else {
                    written.to_string()
                };
            }
            match scope.rfind('.') {
                Some(i) => scope.truncate(i),
                None if scope.is_empty() => break,
                None => scope.clear(),
            }
        }
        // Not declared here (imported type): keep as written.
        written.to_string()
    }

    fn strip_package(&self, name: &str) -> String {
        if !self.package.is_empty() {
            if let Some(rest) = name.strip_prefix(self.package) {
                if let Some(rest) = rest.strip_prefix('.') {
                    return rest.to_string();
                }
            }
        }
        name.to_string()
    }

    fn type_ref(&self, scope: &str, t: &ProtoType) -> ir::TypeRefIr {
        let scalar = |s: &str| ir::TypeRefIr {
            path: vec![s.to_string()],
        };
        match t {
            ProtoType::Double => scalar("float64"),
            ProtoType::Float => scalar("float32"),
            ProtoType::Int32 => scalar("int32"),
            ProtoType::Int64 => scalar("int64"),
            ProtoType::UInt32 => scalar("uint32"),
            ProtoType::UInt64 => scalar("uint64"),
            ProtoType::SInt32 => scalar("sint32"),
            ProtoType::SInt64 => scalar("sint64"),
            ProtoType::Fixed32 => scalar("fixed32"),
            ProtoType::Fixed64 => scalar("fixed64"),
            ProtoType::SFixed32 => scalar("sfixed32"),
            ProtoType::SFixed64 => scalar("sfixed64"),
            ProtoType::Bool => scalar("bool"),
            ProtoType::String => scalar("string"),
            ProtoType::Bytes => scalar("bytes"),
            ProtoType::Message { name } | ProtoType::Enum { name } => {
                let written = name.join(".");
                ir::TypeRefIr {
                    path: self
                        .resolve(scope, &written)
                        .split('.')
                        .map(str::to_string)
                        .collect(),
                }
            }
            // Only reachable for a map nested in a map, which protobuf forbids.
            ProtoType::Map { .. } => scalar("bytes"),
        }
    }
}

// ---------------------------------------------------------------------------
// Messages, fields, enums, services
// ---------------------------------------------------------------------------

fn bool_option(opts: &[OptionDecl], name: &str) -> bool {
    opts.iter()
        .any(|o| o.name == name && o.value == OptionValue::Bool(true))
}

fn lower_field(
    cx: &Cx<'_>,
    scope: &str,
    f: &Field,
    features: Features,
    in_oneof: bool,
) -> Result<ir::FieldIr, ProtoError> {
    let features = features.with_options(&f.options)?;
    let mut annotations = Vec::new();
    let (label, presence) = match (&f.field_type, &f.label) {
        (ProtoType::Map { key, value }, _) => (
            ir::FieldLabelIr::Map {
                key: cx.type_ref(scope, key),
                value: cx.type_ref(scope, value),
            },
            ir::Presence::Implicit,
        ),
        (t, FieldLabel::Repeated) => (
            ir::FieldLabelIr::Repeated(cx.type_ref(scope, t)),
            ir::Presence::Implicit,
        ),
        (t, label) => {
            let presence = if in_oneof {
                ir::Presence::Implicit
            } else {
                match label {
                    FieldLabel::Optional => ir::Presence::Explicit,
                    FieldLabel::Required => {
                        annotations.push(flag("proto_required"));
                        ir::Presence::Explicit
                    }
                    _ => match features.presence {
                        Presence::Explicit => ir::Presence::Explicit,
                        Presence::Implicit => ir::Presence::Implicit,
                        Presence::LegacyRequired => {
                            annotations.push(flag("proto_required"));
                            ir::Presence::Explicit
                        }
                    },
                }
            };
            (ir::FieldLabelIr::Singular(cx.type_ref(scope, t)), presence)
        }
    };
    if bool_option(&f.options, "deprecated") {
        annotations.push(flag("deprecated"));
    }
    Ok(ir::FieldIr {
        id: f.number,
        name: f.name.clone(),
        label,
        presence,
        annotations,
        span: Default::default(),
    })
}

fn flag(name: &str) -> ir::AnnotationIr {
    ir::AnnotationIr {
        name: name.to_string(),
        args: Vec::new(),
    }
}

fn lower_message(
    cx: &Cx<'_>,
    msg: &Message,
    parent: &str,
    features: Features,
) -> Result<ir::MessageIr, ProtoError> {
    let full = join(parent, &msg.name);
    let features = features.with_options(&msg.options)?;
    let mut annotations = Vec::new();
    if bool_option(&msg.options, "deprecated") {
        annotations.push(flag("deprecated"));
    }
    let mut out = ir::MessageIr {
        name: msg.name.clone(),
        fields: Vec::new(),
        oneofs: Vec::new(),
        messages: Vec::new(),
        enums: Vec::new(),
        reserved: msg.reserved.iter().cloned().map(lower_reserved).collect(),
        annotations,
        span: Default::default(),
    };
    for f in &msg.fields {
        out.fields.push(lower_field(cx, &full, f, features, false)?);
    }
    for o in &msg.oneofs {
        let mut fields = Vec::new();
        for f in &o.fields {
            fields.push(lower_field(cx, &full, f, features, true)?);
        }
        out.oneofs.push(ir::OneofIr {
            name: o.name.clone(),
            fields,
            annotations: Vec::new(),
            span: Default::default(),
        });
    }
    for child in &msg.messages {
        out.messages
            .push(lower_message(cx, child, &full, features)?);
    }
    for en in &msg.enums {
        out.enums.push(lower_enum(en, features)?);
    }
    Ok(out)
}

fn lower_enum(en: &Enum, features: Features) -> Result<ir::EnumIr, ProtoError> {
    let features = features.with_options(&en.options)?;
    let mut seen = HashSet::new();
    let values = en
        .values
        .iter()
        .map(|v| ir::EnumValueIr {
            name: v.name.clone(),
            number: v.number,
            alias: !seen.insert(v.number),
        })
        .collect();
    let mut annotations = Vec::new();
    if bool_option(&en.options, "deprecated") {
        annotations.push(flag("deprecated"));
    }
    Ok(ir::EnumIr {
        name: en.name.clone(),
        values,
        open: features.open_enums,
        annotations,
        span: Default::default(),
    })
}

fn lower_service(cx: &Cx<'_>, svc: &Service) -> ir::ServiceIr {
    let ty = |path: &[String]| ir::TypeRefIr {
        path: cx
            .resolve("", &path.join("."))
            .split('.')
            .map(str::to_string)
            .collect(),
    };
    ir::ServiceIr {
        name: svc.name.clone(),
        methods: svc
            .methods
            .iter()
            .map(|m| ir::MethodIr {
                name: m.name.clone(),
                request: ty(&m.request_type),
                request_streaming: m.request_streaming,
                response: ty(&m.response_type),
                response_streaming: m.response_streaming,
                annotations: if bool_option(&m.options, "deprecated") {
                    vec![flag("deprecated")]
                } else {
                    Vec::new()
                },
            })
            .collect(),
        annotations: if bool_option(&svc.options, "deprecated") {
            vec![flag("deprecated")]
        } else {
            Vec::new()
        },
        span: Default::default(),
    }
}

fn lower_reserved(r: crate::proto_ast::Reserved) -> ir::ReservedIr {
    ir::ReservedIr {
        ids: r
            .ids
            .into_iter()
            .map(|id| match id {
                ReservedId::Single(n) => ir::ReservedIdIr::Single(n),
                ReservedId::Range(a, b) => ir::ReservedIdIr::Range(a, b),
            })
            .collect(),
        names: r.names,
    }
}

// ---------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------

fn find_ast<'a>(roots: &'a [Message], full: &str) -> Option<&'a Message> {
    let mut parts = full.split('.');
    let first = parts.next()?;
    let mut cur = roots.iter().find(|m| m.name == first)?;
    for p in parts {
        cur = cur.messages.iter().find(|m| m.name == p)?;
    }
    Some(cur)
}

fn find_ir_mut<'a>(pkg: &'a mut ir::PackageIr, full: &str) -> Option<&'a mut ir::MessageIr> {
    let mut parts = full.split('.');
    let first = parts.next()?;
    let mut cur = pkg.messages.iter_mut().find(|m| m.name == first)?;
    for p in parts {
        cur = cur.messages.iter_mut().find(|m| m.name == p)?;
    }
    Some(cur)
}

fn reserved_covers(reserved: &[ir::ReservedIr], id: u32) -> bool {
    reserved.iter().flat_map(|r| r.ids.iter()).any(|r| match r {
        ir::ReservedIdIr::Single(n) => *n == id,
        ir::ReservedIdIr::Range(a, b) => (*a..=*b).contains(&id),
    })
}

fn merge_extend(
    cx: &Cx<'_>,
    pkg: &mut ir::PackageIr,
    roots: &[Message],
    ext: &Extend,
    scope: &str,
    features: Features,
    report: &mut LowerReport,
) -> Result<(), ProtoError> {
    let written = ext.message_type.join(".");
    let target = cx.resolve(scope, &written);
    let is_local = cx.declared.get(&target) == Some(&Declared::Message);
    if !is_local {
        for f in &ext.fields {
            report
                .dropped_extensions
                .push(format!("{written}.{}", f.name));
        }
        return Ok(());
    }
    let ranges = find_ast(roots, &target)
        .map(|m| m.extension_ranges.clone())
        .unwrap_or_default();
    let mut lowered = Vec::new();
    for f in &ext.fields {
        // Extensions are optional by nature; proto2 `optional` is explicit.
        let mut field = lower_field(cx, scope, f, features, false)?;
        if matches!(field.label, ir::FieldLabelIr::Singular(_)) {
            field.presence = ir::Presence::Explicit;
        }
        if !ranges.is_empty() && !ranges.iter().any(|(a, b)| (*a..=*b).contains(&f.number)) {
            return Err(ProtoError::ExtensionConflict(f.number));
        }
        lowered.push(field);
    }
    let msg = find_ir_mut(pkg, &target)
        .ok_or(ProtoError::Unsupported("extendee vanished during lowering"))?;
    for field in lowered {
        let taken_id = msg
            .fields
            .iter()
            .chain(msg.oneofs.iter().flat_map(|o| o.fields.iter()))
            .any(|x| x.id == field.id)
            || reserved_covers(&msg.reserved, field.id);
        if taken_id {
            return Err(ProtoError::ExtensionConflict(field.id));
        }
        let taken_name = msg
            .fields
            .iter()
            .chain(msg.oneofs.iter().flat_map(|o| o.fields.iter()))
            .any(|x| x.name == field.name);
        if taken_name {
            return Err(ProtoError::DuplicateField(field.name));
        }
        msg.fields.push(field);
    }
    Ok(())
}

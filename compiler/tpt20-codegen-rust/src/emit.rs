//! The Rust source emitter (spec §12).
//!
//! Split across focused methods; [`Emitter::generate`] assembles the module.

use tpt20_ir as ir;

use crate::expr;
use crate::model;
use crate::model::{Model, TypeKind};
use crate::naming;
use crate::scalars;
use crate::scalars::{scalar_info, PackKind};
use crate::CodegenOptions;

/// Per-message emission context.
struct MsgCtx {
    /// Flat Rust type name, e.g. `User` or `Outer_Inner`.
    flat: String,
    /// Scope path of dotted names.
    scope: Vec<String>,
    /// All known field ids on the wire for this message (incl. oneof members).
    known_ids: Vec<u32>,
    /// Oneof groups as member-id slices.
    oneof_groups: Vec<Vec<u32>>,
    /// Map-entry field ids.
    map_ids: Vec<u32>,
}

pub(crate) struct Emitter<'a> {
    pkg: &'a ir::PackageIr,
    opts: &'a CodegenOptions,
    model: Model,
    out: String,
}

impl<'a> Emitter<'a> {
    pub(crate) fn new(pkg: &'a ir::PackageIr, opts: &'a CodegenOptions) -> Emitter<'a> {
        let model = Model::build(pkg);
        Emitter {
            pkg,
            opts,
            model,
            out: String::new(),
        }
    }

    pub(crate) fn generate(mut self) -> String {
        self.header();
        self.support_mod();
        // Clone the top-level lists so recursive &mut self calls stay simple.
        let enums = self.pkg.enums.clone();
        let messages = self.pkg.messages.clone();
        let top_scope: Vec<String> = Vec::new();
        for e in &enums {
            self.emit_enum(&top_scope, e);
        }
        for m in &messages {
            self.emit_message(&top_scope, &m);
        }
        if self.opts.builders && !self.pkg.messages.is_empty() {
            self.emit_build_error();
        }
        self.out
    }

    /// Resolves a type reference to its flat Rust name and kind.
    ///
    /// Scalars are recognized lexically (they are not registry entries);
    /// messages/enums come from the model.
    fn resolve_ref(&self, scope: &[String], path: &[String]) -> (String, TypeKind) {
        if path.len() == 1 {
            if let Some(info) = path.first().and_then(|p| scalars::scalar_info(p)) {
                return (info.rust.to_string(), TypeKind::Scalar(info));
            }
        }
        match self.model.resolve(scope, path) {
            Some((flat, kind)) => (flat.to_string(), kind),
            None => ("()".to_string(), TypeKind::Message),
        }
    }

    /// Returns true if the message or any of its transitive fields borrow
    /// string/bytes payloads, which forces the view struct to carry a lifetime.
    fn message_needs_lifetime(&self, msg: &ir::MessageIr) -> bool {
        fn field_needs(f: &ir::FieldIr) -> bool {
            match &f.label {
                ir::FieldLabelIr::Singular(t) => {
                    model::is_scalar_path(&t.path)
                        && matches!(t.path[0].as_str(), "string" | "bytes")
                }
                ir::FieldLabelIr::Repeated(t) => {
                    model::is_scalar_path(&t.path)
                        && matches!(t.path[0].as_str(), "string" | "bytes")
                }
                ir::FieldLabelIr::Map { value, .. } => {
                    model::is_scalar_path(&value.path)
                        && matches!(value.path[0].as_str(), "string" | "bytes")
                }
            }
        }
        msg.fields.iter().any(field_needs)
            || msg.oneofs.iter().any(|o| o.fields.iter().any(field_needs))
            || msg.messages.iter().any(|m| self.message_needs_lifetime(m))
    }

    /// Owned Rust type for a referenced type relative to `scope`.
    fn owned_type(&self, scope: &[String], path: &[String]) -> String {
        if model::is_scalar_path(path) {
            return scalar_info(path[0].as_str())
                .map(|i| i.rust.to_string())
                .unwrap_or_else(|| "()".to_string());
        }
        self.resolve_ref(scope, path).0
    }

    /// Borrowed Rust type for views (string/bytes become references).
    fn view_type(&self, scope: &[String], path: &[String]) -> String {
        if model::is_scalar_path(path) {
            return expr::view_rust_type(path[0].as_str()).to_string();
        }
        let base = self.resolve_ref(scope, path).0;
        format!("{base}View<'a>")
    }

    fn type_needs_lifetime(&self, scope: &[String], path: &[String]) -> bool {
        if model::is_scalar_path(path) {
            return matches!(path[0].as_str(), "string" | "bytes");
        }
        if let Some(msg) = self.find_message_ir(scope, path) {
            self.message_needs_lifetime(msg)
        } else {
            true
        }
    }

    fn find_message_ir(&self, scope: &[String], path: &[String]) -> Option<&'a ir::MessageIr> {
        if path.is_empty() {
            return None;
        }
        if let Some((flat, _)) = self.model.resolve(scope, path) {
            return self.find_by_flat_name(&self.pkg.messages, flat);
        }
        None
    }

    fn find_by_flat_name(&self, messages: &'a [ir::MessageIr], flat: &str) -> Option<&'a ir::MessageIr> {
        for m in messages {
            let self_flat = naming::flat_type_name(&[], &m.name);
            if self_flat == flat {
                return Some(m);
            }
            if let Some(nested) = self.find_by_flat_name(&m.messages, flat) {
                return Some(nested);
            }
        }
        None
    }

    fn header(&mut self) {
        let fp = self.pkg.fingerprint.clone().unwrap_or_default();
        let core = &self.opts.core_crate;
        let json = &self.opts.json_crate;
        self.out.push_str(&format!(
            r#"// @generated by tpt20-codegen-rust -- DO NOT EDIT.
// Schema package: {pkg}
// Schema fingerprint: {fp}
//
// Required dependencies in the consuming crate:
//   {core} = "..."   (aliased below as __core)
//   {json} = "..."   (aliased below as __json)
//
// This file is a Rust module: include it with `mod <name>;`,
// `include!`, or the build-system integration of your choice.
use std::collections::BTreeMap;
use {core} as __core;
use {core}::scalar as __scalar;
use {json} as __json;
"#,
            pkg = self.pkg.name.as_deref().unwrap_or("<none>"),
        ));
    }

    fn support_mod(&mut self) {
        self.out.push_str(
            r#"
/// Hidden helpers shared by generated code.
#[doc(hidden)]
pub mod __support {
    use super::__core;
    use super::__json;

    /// Reads a sign-extended varint as an i32 (enum / int32 wire form).
    pub fn wire_i32(v: &__core::Value) -> Result<i32, __core::DecodeError> {
        __core::scalar::decode_signed(v).map(|x| x as i32)
    }

    pub fn as_i32(v: &__json::Value) -> Result<i32, __json::JsonError> {
        __json::as_i64(v)?
            .try_into()
            .map_err(|_| __json::JsonError::TypeMismatch { expected: "int32" })
    }

    pub fn as_u32(v: &__json::Value) -> Result<u32, __json::JsonError> {
        __json::as_u64(v)?
            .try_into()
            .map_err(|_| __json::JsonError::TypeMismatch { expected: "uint32" })
    }
}
"#,
        );
    }

    fn emit_build_error(&mut self) {
        self.out.push_str(
            r#"
/// Errors produced by generated builders (spec §12.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// A length-bounded field exceeded its `@max_len`.
    MaxLenExceeded {
        /// Offending field name.
        field: &'static str,
        /// Configured maximum.
        max: usize,
    },
    /// A length-bounded field was shorter than its `@min_len`.
    MinLenViolation {
        /// Offending field name.
        field: &'static str,
        /// Configured minimum.
        min: usize,
    },
    /// An integer field fell outside its `@range`.
    OutOfRange {
        /// Offending field name.
        field: &'static str,
    },
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::MaxLenExceeded { field, max } => {
                write!(f, "field `{field}` exceeds @max_len({max})")
            }
            BuildError::MinLenViolation { field, min } => {
                write!(f, "field `{field}` is shorter than @min_len({min})")
            }
            BuildError::OutOfRange { field } => {
                write!(f, "field `{field}` is outside its @range")
            }
        }
    }
}

impl std::error::Error for BuildError {}
"#,
        );
    }

    fn msg_ctx(&self, scope: &[String], msg: &ir::MessageIr) -> MsgCtx {
        let mut known_ids: Vec<u32> = msg.fields.iter().map(|f| f.id).collect();
        let mut oneof_groups = Vec::new();
        let mut map_ids = Vec::new();
        for o in &msg.oneofs {
            let ids: Vec<u32> = o.fields.iter().map(|f| f.id).collect();
            known_ids.extend_from_slice(&ids);
            oneof_groups.push(ids);
        }
        for f in &msg.fields {
            if matches!(f.label, ir::FieldLabelIr::Map { .. }) {
                map_ids.push(f.id);
            }
        }
        MsgCtx {
            flat: naming::flat_type_name(scope, &msg.name),
            scope: {
                let mut s = scope.to_vec();
                s.push(msg.name.clone());
                s
            },
            known_ids,
            oneof_groups,
            map_ids,
        }
    }

    fn emit_oneof_enum(&mut self, scope: &[String], msg: &ir::MessageIr, o: &ir::OneofIr, view: bool) {
        let oty = if view {
            format!("{}{}<'a>", naming::flat_type_name(scope, &msg.name), naming::pascal(&o.name))
        } else {
            format!("{}{}", naming::flat_type_name(scope, &msg.name), naming::pascal(&o.name))
        };
        let mut s = String::new();
        if view {
            s.push_str(&format!(
                "\n/// Borrowed oneof view for `{}`.\n#[derive(Debug, Clone, PartialEq)]\npub enum {oty}<'a> {{\n",
                o.name
            ));
        } else {
            s.push_str(&format!(
                "\n/// Oneof `{}`.\n#[derive(Debug, Clone, PartialEq)]\npub enum {oty} {{\n",
                o.name
            ));
        }
        for mf in &o.fields {
            let variant = naming::sanitize_ident(&naming::pascal(&mf.name));
            let t = mf.label.unwrap_type();
            let vty = if view {
                self.view_type(scope, &t.path)
            } else {
                self.owned_type(scope, &t.path)
            };
            s.push_str(&format!("    /// Field id {}.\n    {variant}({vty}),\n", mf.id));
        }
        s.push_str("}\n");
        self.out.push_str(&s);
    }

    /// Emits a schema enum as a Rust enum (spec §12.4).
    fn emit_enum(&mut self, scope: &[String], e: &ir::EnumIr) {
        let flat = naming::flat_type_name(scope, &e.name);
        // First non-alias value with number 0 becomes the default; else first.
        let default_number = e
            .values
            .iter()
            .find(|v| v.number == 0 && !v.alias)
            .map(|v| v.number)
            .or_else(|| e.values.first().map(|v| v.number))
            .unwrap_or(0);

        self.out.push_str(&format!(
            "\n/// Generated enum `{flat}` ({}).\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum {flat} {{\n",
            if e.open { "open" } else { "closed" }
        ));
        for v in &e.values {
            if v.alias {
                continue; // Aliases stay representable by number, not variant.
            }
            let variant = naming::sanitize_ident(&naming::pascal(&v.name));
            if e.open {
                self.out
                    .push_str(&format!("    /// Value `{}` ({}).\n    {variant},\n", v.name, v.number));
            } else {
                self.out.push_str(&format!(
                    "    /// Value `{}` ({}).\n    {variant} = {},\n",
                    v.name, v.number, v.number
                ));
            }
        }
        if e.open {
            self.out.push_str(
                "    /// Unknown value preserved per open-enum semantics.\n    Unknown(i32),\n",
            );
        }
        self.out.push_str("}\n");

        self.out.push_str(&format!("impl {flat} {{\n"));
        if e.open {
            self.out.push_str(&format!(
                r#"    /// Maps a wire integer to the enum, capturing unknowns.
    pub fn from_i32(v: i32) -> Self {{
        match v {{
{arms}            other => Self::Unknown(other),
        }}
    }}

    /// Maps the enum back to its wire integer.
    pub fn to_i32(self) -> i32 {{
        match self {{
{to_arms}            Self::Unknown(v) => v,
        }}
    }}
}}

impl Default for {flat} {{
    fn default() -> Self {{
        Self::from_i32({default_number})
    }}
}}
"#,
                arms = e
                    .values
                    .iter()
                    .filter(|v| !v.alias)
                    .map(|v| format!(
                        "            {} => {}::{},\n",
                        v.number,
                        flat,
                        naming::sanitize_ident(&naming::pascal(&v.name))
                    ))
                    .collect::<String>(),
                to_arms = e
                    .values
                    .iter()
                    .filter(|v| !v.alias)
                    .map(|v| format!(
                        "            Self::{} => {},\n",
                        naming::sanitize_ident(&naming::pascal(&v.name)),
                        v.number
                    ))
                    .collect::<String>(),
            ));
        } else {
            self.out.push_str(&format!(
                r#"    /// Maps a wire integer; closed enums reject unknown values.
    pub fn from_i32(v: i32) -> Result<Self, __core::DecodeError> {{
        match v {{
{arms}            _ => Err(__core::DecodeError::InvalidEnumValue(v)),
        }}
    }}

    /// Maps the enum back to its wire integer.
    pub fn to_i32(self) -> i32 {{
        self as i32
    }}
}}

impl Default for {flat} {{
    fn default() -> Self {{
        Self::from_i32({default_number}).unwrap_or_default_or_first()
    }}
}}
"#,
                arms = e
                    .values
                    .iter()
                    .filter(|v| !v.alias)
                    .map(|v| format!(
                        "            {} => Ok({}::{}),\n",
                        v.number,
                        flat,
                        naming::sanitize_ident(&naming::pascal(&v.name))
                    ))
                    .collect::<String>(),
            ));
        }
        if !e.open {
            // Replace the placeholder call above with a concrete default.
            let first_variant = e
                .values
                .iter()
                .find(|v| !v.alias && v.number == default_number)
                .or_else(|| e.values.iter().find(|v| !v.alias))
                .map(|v| naming::sanitize_ident(&naming::pascal(&v.name)))
                .unwrap_or_else(|| "_".to_string());
            let needle = format!(
                "Self::from_i32({default_number}).unwrap_or_default_or_first()"
            );
            let replacement = format!(
                "Self::from_i32({default_number}).unwrap_or(Self::{first_variant})"
            );
            self.out = self.out.replacen(&needle, &replacement, 1);
        }

        // JSON helpers (spec \u{a7}14.2 enums by name or number).
        let mut j = String::new();
        j.push_str(&format!("impl {flat} {{\n"));
        let name_arms = e
            .values
            .iter()
            .filter(|v| !v.alias)
            .map(|v| {
                format!(
"            {flat}::{} => __json::Value::String({:?}.to_string()),\n",
                    naming::sanitize_ident(&naming::pascal(&v.name)),
                    v.name
                )
            })
            .collect::<String>();
        if e.open {
            j.push_str(&format!(
"    /// JSON representation (name; unknown values as numbers).\n    pub fn json_name(v: &Self) -> __json::Value {{\n        match v {{\n{name_arms}            Self::Unknown(n) => __json::Value::from(*n),\n        }}\n    }}\n"
            ));
        } else {
            j.push_str(&format!(
"    /// JSON representation (name).\n    pub fn json_name(v: &Self) -> __json::Value {{\n        match v {{\n{name_arms}        }}\n    }}\n"
            ));
        }
        let parse_arms = e
            .values
            .iter()
            .filter(|v| !v.alias)
            .map(|v| {
                format!(
"                {:?} => Ok({flat}::{}),\n",
                    v.name,
                    naming::sanitize_ident(&naming::pascal(&v.name))
                )
            })
            .collect::<String>();
        if e.open {
            j.push_str(&format!(
"    /// Parses from a JSON string (name) or number.\n    pub fn from_json(v: &__json::Value) -> Result<Self, __json::JsonError> {{\n        if let Some(sv) = v.as_str() {{\n            match sv {{\n{parse_arms}                other => Err(__json::JsonError::InvalidEnum(other.to_string())),\n            }}\n        }} else {{\n            let n = __json::as_i64(v)? as i32;\n            Ok(Self::from_i32(n))\n        }}\n    }}\n}}\n"
            ));
        } else {
            j.push_str(&format!(
"    /// Parses from a JSON string (name) or number.\n    pub fn from_json(v: &__json::Value) -> Result<Self, __json::JsonError> {{\n        if let Some(sv) = v.as_str() {{\n            match sv {{\n{parse_arms}                other => Err(__json::JsonError::InvalidEnum(other.to_string())),\n            }}\n        }} else {{\n            let n = __json::as_i64(v)? as i32;\n            Self::from_i32(n)\n                .map_err(|_| __json::JsonError::InvalidEnum(n.to_string()))\n        }}\n    }}\n}}\n"
            ));
        }
        self.out.push_str(&j);
    }

    /// Emits a message: owned struct, views, wire methods, JSON, builder.
    fn emit_message(&mut self, scope: &[String], msg: &ir::MessageIr) {
        let ctx = self.msg_ctx(scope, msg);
        let inner_scope = ctx.scope.clone();

        // ---- Owned struct -------------------------------------------------
        let mut s = String::new();
        s.push_str(&format!(
            "\n/// Generated message `{}`.\n#[derive(Debug, Clone, PartialEq, Default)]\npub struct {} {{\n",
            ctx.flat, ctx.flat
        ));
        self.push_struct_fields(scope, msg, &ctx, &mut s);
        self.out.push_str(&s);

        // ---- Owned oneof enums -------------------------------------------------
        for o in &msg.oneofs {
            self.emit_oneof_enum(scope, msg, o, false);
        }

        // ---- Methods --------------------------------------------------------
        self.emit_encode_impls(scope, msg, &ctx);
        self.emit_decode_owned(scope, msg, &ctx);
        self.emit_view(scope, msg, &ctx);
        self.emit_json(scope, msg, &ctx);
        if self.opts.builders {
            self.emit_builder(scope, msg, &ctx);
        }

        // ---- Nested declarations ---------------------------------------------
        let enums = msg.enums.clone();
        for e in &enums {
            self.emit_enum(&inner_scope, &e);
        }
        let messages = msg.messages.clone();
        for m in &messages {
            self.emit_message(&inner_scope, m);
        }
    }

    /// Appends field declarations of the owned struct.
    fn push_struct_fields(
        &self,
        scope: &[String],
        msg: &ir::MessageIr,
        ctx: &MsgCtx,
        out: &mut String,
    ) {
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            let ty = self.struct_field_type(scope, f);
            out.push_str(&format!(
                "    /// Field id {}: `{}`.\n    pub {fname}: {ty},\n",
                f.id, f.name
            ));
        }
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            let oty = format!("{}{}", ctx.flat, naming::pascal(&o.name));
            out.push_str(&format!(
                "    /// Oneof `{}`.\n    pub {oname}: Option<{oty}>,\n",
                o.name
            ));
        }
        out.push_str("    /// Preserved unknown fields (spec \u{a7}9.9); re-encoded verbatim.\n    #[doc(hidden)]\n    pub unknown_fields: __core::RawMessage,\n}\n");
    }

    /// Rust type for a regular (non-oneof) struct field.
    fn struct_field_type(&self, scope: &[String], f: &ir::FieldIr) -> String {
        match &f.label {
            ir::FieldLabelIr::Singular(t) => {
                let kind = self.resolve_ref(scope, &t.path).1;
                let base = self.owned_type(scope, &t.path);
                match kind {
                    TypeKind::Message => format!("Option<{base}>"),
                    _ => match f.presence {
                        ir::Presence::Explicit => format!("Option<{base}>"),
                        ir::Presence::Implicit => base,
                    },
                }
            }
            ir::FieldLabelIr::Repeated(t) => {
                format!("Vec<{}>", self.owned_type(scope, &t.path))
            }
            ir::FieldLabelIr::Map { key, value } => format!(
                "BTreeMap<{}, {}>",
                self.owned_type(scope, &key.path),
                self.owned_type(scope, &value.path)
            ),
        }
    }

    /// Condition guarding implicit-presence emission (defaults are skipped).
    fn skip_cond(scalar: &str, v: &str) -> String {
        let raw = match scalar {
            "bool" => format!("*{v}"),
            "float32" | "float64" => format!("*{v} != 0.0"),
            "string" | "bytes" => format!("!{v}.is_empty()"),
            _ => format!("*{v} != 0"),
        };
        raw
    }

    /// Emits `to_raw`, `encode`, and `encode_canonical`.
    fn emit_encode_impls(&mut self, scope: &[String], msg: &ir::MessageIr, ctx: &MsgCtx) {
        let mut b = String::new();
        b.push_str(&format!("impl {} {{\n", ctx.flat));
        b.push_str(
            "    /// Converts to the neutral raw field model (reflection-friendly).\n    pub fn to_raw(&self) -> __core::RawMessage {\n        let mut raw = __core::RawMessage::new();\n",
        );
        self.push_to_raw_fields(scope, msg, ctx, &mut b);
        self.push_to_raw_oneofs(scope, msg, ctx, &mut b);
        b.push_str("        raw.extend(self.unknown_fields.clone());\n        raw\n    }\n");

        let groups = ctx
            .oneof_groups
            .iter()
            .map(|g| {
                format!(
                    "&[{}]",
                    g.iter().map(|i| format!("{i}u32")).collect::<Vec<_>>().join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let maps = ctx
            .map_ids
            .iter()
            .map(|i| format!("{i}u32"))
            .collect::<Vec<_>>()
            .join(", ");
        b.push_str(&format!(
r#"    /// Encodes to the native binary wire format (spec \u{{a7}}9).
    pub fn encode(&self) -> Vec<u8> {{
        self.to_raw().encode().unwrap_or_default()
    }}

    /// Encodes in canonical deterministic form (spec \u{{a7}}9.10): oneof
    /// last-wins reduction, key-sorted map entries, total field order.
    pub fn encode_canonical(&self) -> Vec<u8> {{
        let mut raw = self.to_raw();
        raw.canonical_reduce_oneofs(&[{groups}]);
        raw.canonical_sort_map_entries(&[{maps}]);
        raw.encode_canonical().unwrap_or_default()
    }}
}}
"#,
            groups = groups,
            maps = maps,
        ));
        self.out.push_str(&b);
    }

    /// Emits per-field pushes into the raw message.
    fn push_to_raw_fields(
        &self,
        scope: &[String],
        msg: &ir::MessageIr,
        _ctx: &MsgCtx,
        b: &mut String,
    ) {
        use ir::FieldLabelIr;
        use ir::Presence::{Explicit, Implicit};
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            match &f.label {
                FieldLabelIr::Singular(t) => {
                    let kind = self.resolve_ref(scope, &t.path).1;
                    match kind {
                        TypeKind::Scalar(info) => {
                            let enc = expr::enc_value(&t.path[0], "v");
                            let class = class_name(info.class);
                            match f.presence {
                                Explicit => {
                                    b.push_str(&format!(
"        if let Some(v) = &self.{fname} {{\n            raw.push(__core::Field::new({id}, {class}, {enc}));\n        }}\n",
                                        id = f.id
                                    ));
                                }
                                Implicit => {
                                    let cond =
                                        Self::skip_cond(&t.path[0], "v");
                                    b.push_str(&format!(
"        let v = &self.{fname};\n        if {cond} {{\n            raw.push(__core::Field::new({id}, {class}, {enc}));\n        }}\n",
                                        id = f.id
                                    ));
                                }
                            }
                        }
                        TypeKind::Enum { .. } => {
                            let cls = class_name(crate::WireClass::Varint);
                            match f.presence {
                                Explicit => b.push_str(&format!(
"        if let Some(v) = &self.{fname} {{\n            raw.push(__core::Field::new({id}, {cls}, __core::Value::Varint((v.to_i32() as u64))));\n        }}\n",
                                    id = f.id
                                )),
                                Implicit => b.push_str(&format!(
"        if self.{fname}.to_i32() != 0 {{\n            raw.push(__core::Field::new({id}, {cls}, __core::Value::Varint((self.{fname}.to_i32() as u64))));\n        }}\n",
                                    id = f.id
                                )),
                            }
                        }
                        TypeKind::Message => {
                            let cls = class_name(crate::WireClass::Len);
                            b.push_str(&format!(
"        if let Some(v) = &self.{fname} {{\n            raw.push(__core::Field::new({id}, {cls}, __core::Value::Len(v.encode())));\n        }}\n",
                                id = f.id
                            ));
                        }
                    }
                }
                FieldLabelIr::Repeated(t) => {
                    self.push_repeated_encode(scope, f, t.path[0].as_str(), &fname, b);
                }
                FieldLabelIr::Map { key, value } => {
                    self.push_map_encode(scope, f, key.path[0].as_str(), value, &fname, b);
                }
            }
        }
    }

    fn push_repeated_encode(
        &self,
        scope: &[String],
        f: &ir::FieldIr,
        scalar: &str,
        fname: &str,
        b: &mut String,
    ) {
        let id = f.id;
        let kind = self.resolve_ref(scope, f.label.unwrap_type().path.as_slice()).1;
        match kind {
            TypeKind::Scalar(info) => match info.pack {
                PackKind::NotPackable => {
                    let enc = expr::enc_value(scalar, "v");
                    b.push_str(&format!(
"        for v in &self.{fname} {{\n            raw.push(__core::Field::new({id}, {}, {enc}));\n        }}\n",
                        class_name(crate::WireClass::Len),
                    ));
                }
                pack => {
                    let word = expr::to_wire_word(scalar, "*v");
                    let pfn = packed_encode_fn(pack);
                    b.push_str(&format!(
"        if !self.{fname}.is_empty() {{\n            let words: Vec<_> = self.{fname}.iter().map(|v| {word}).collect();\n            raw.push(__core::Field::new({id}, {}, {pfn}(&words)));\n        }}\n",
                        class_name(crate::WireClass::Len),
                    ));
                }
            },
            TypeKind::Enum { .. } => b.push_str(&format!(
"        if !self.{fname}.is_empty() {{\n            let words: Vec<u64> = self.{fname}.iter().map(|v| v.to_i32() as u64).collect();\n            raw.push(__core::Field::new({id}, {}, __scalar::encode_packed_varints(&words)));\n        }}\n",
                class_name(crate::WireClass::Len),
            )),
            TypeKind::Message => b.push_str(&format!(
"        for v in &self.{fname} {{\n            raw.push(__core::Field::new({id}, {}, __core::Value::Len(v.encode())));\n        }}\n",
                class_name(crate::WireClass::Len),
            )),
        }
    }

    /// Emits the map-entry encoding loop for a map field.
    fn push_map_encode(
        &self,
        scope: &[String],
        f: &ir::FieldIr,
        key_scalar: &str,
        value: &ir::TypeRefIr,
        fname: &str,
        b: &mut String,
    ) {
        let kinfo = scalar_info(key_scalar).expect("map keys must be scalar");
        let kenc = expr::enc_value(key_scalar, "k");
        let vkind = self.resolve_ref(scope, &value.path).1;
        let (vclass, venc) = match vkind {
            TypeKind::Scalar(info) => (class_name(info.class), expr::enc_value(&value.path[0], "v")),
            TypeKind::Enum { .. } => (
                class_name(crate::WireClass::Varint),
                "__core::Value::Varint((v.to_i32() as u64))".to_string(),
            ),
            TypeKind::Message => (
                class_name(crate::WireClass::Len),
                "__core::Value::Len(v.encode())".to_string(),
            ),
        };
        b.push_str(&format!(
r#"        for (k, v) in &self.{fname} {{
            let mut entry = __core::RawMessage::new();
            entry.push(__core::Field::new(1, {kclass}, {kenc}));
            entry.push(__core::Field::new(2, {vclass}, {venc}));
            raw.push(__core::Field::new({id}, {len}, __core::Value::Len(entry.encode().unwrap_or_default())));
        }}
"#,
            kclass = class_name(kinfo.class),
            len = class_name(crate::WireClass::Len),
            id = f.id,
        ));
    }

    /// Emits oneof encoding (each member is a normal wire field; last wins).
    fn push_to_raw_oneofs(
        &self,
        scope: &[String],
        msg: &ir::MessageIr,
        ctx: &MsgCtx,
        b: &mut String,
    ) {
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            let oty = format!("{}{}", ctx.flat, naming::pascal(&o.name));
            b.push_str(&format!("        match &self.{oname} {{\n"));
            for mf in &o.fields {
                let variant = naming::sanitize_ident(&naming::pascal(&mf.name));
                b.push_str(&self.oneof_push_arm(scope, &oty, &variant, mf));
            }
            b.push_str("            None => {}\n        }\n");
        }
    }

    /// Builds the `to_raw` arm for one oneof member.
    fn oneof_push_arm(
        &self,
        scope: &[String],
        oneof_ty: &str,
        variant: &str,
        mf: &ir::FieldIr,
    ) -> String {
        let t = mf.label.unwrap_type();
        let kind = self.resolve_ref(scope, &t.path).1;
        match kind {
            TypeKind::Scalar(info) => {
                let enc = expr::enc_value(t.path[0].as_str(), "v");
                format!(
"            Some({oneof_ty}::{variant}(v)) => raw.push(__core::Field::new({}, {}, {enc})),\n",
                    mf.id,
                    class_name(info.class),
                )
            }
            TypeKind::Enum { .. } => format!(
"            Some({oneof_ty}::{variant}(v)) => raw.push(__core::Field::new({}, {}, __core::Value::Varint((v.to_i32() as u64)))),\n",
                mf.id,
                class_name(crate::WireClass::Varint),
            ),
            TypeKind::Message => format!(
"            Some({oneof_ty}::{variant}(v)) => raw.push(__core::Field::new({}, {}, __core::Value::Len(v.encode()))),\n",
                mf.id,
                class_name(crate::WireClass::Len),
            ),
        }
    }

    /// Emits the owned decoding impl block (`decode`, `decode_with_limits`,
    /// `decode_borrowed` entry).
    fn emit_decode_owned(&mut self, scope: &[String], msg: &ir::MessageIr, ctx: &MsgCtx) {
        let known = ctx
            .known_ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let arms = self.decode_arms(scope, msg, &ctx.flat, false);
        let view = format!("{}View", ctx.flat);

        let mut b = String::new();
        b.push_str("impl ");
        b.push_str(&ctx.flat);
        b.push_str(" {\n");
        b.push_str("    /// Field ids part of this message's wire contract.\n");
        b.push_str("    const KNOWN_IDS: &'static [u32] = &[");
        b.push_str(&known);
        b.push_str("];\n\n");
        b.push_str("    /// Depth-bounded recursive decode (spec \u{a7}18.4).\n");
        b.push_str(
            "    fn decode_inner(\n        bytes: &[u8],\n        limits: &__core::DecoderLimits,\n        depth: usize,\n    ) -> Result<Self, __core::DecodeError> {\n",
        );
        b.push_str("        limits.check_depth(depth)?;\n");
        b.push_str(
            "        let raw = __core::RawMessage::decode_filtered(\n            bytes,\n            limits,\n            __core::UnknownFieldPolicy::Preserve,\n            &|id| Self::KNOWN_IDS.contains(&id),\n        )?;\n",
        );
        b.push_str("        let mut out_msg = Self::default();\n");
        b.push_str("        for field in &raw.fields {\n");
        b.push_str("            if !Self::KNOWN_IDS.contains(&field.field_id) {\n");
        b.push_str("                out_msg.unknown_fields.push(field.clone());\n                continue;\n            }\n");
        b.push_str("            match (field.field_id, field.wire_class) {\n");
        b.push_str(&arms);
        b.push_str("                _ => {\n                    return Err(__core::DecodeError::WireClassMismatch {\n                        field_id: field.field_id,\n                    });\n                }\n");
        b.push_str("            }\n        }\n        Ok(out_msg)\n    }\n\n");
        b.push_str(
            "    /// Decodes with explicit resource limits.\n    pub fn decode_with_limits(\n        bytes: &[u8],\n        limits: &__core::DecoderLimits,\n    ) -> Result<Self, __core::DecodeError> {\n        Self::decode_inner(bytes, limits, 1)\n    }\n\n",
        );
        b.push_str(
            "    /// Decodes with the default conservative limits.\n    pub fn decode(bytes: &[u8]) -> Result<Self, __core::DecodeError> {\n        Self::decode_inner(bytes, &__core::DecoderLimits::default(), 1)\n    }\n\n",
        );
        b.push_str("    /// Borrows over `bytes`: zero-copy strings/bytes, nested views recursive.\n");
        b.push_str("    pub fn decode_borrowed(bytes: &[u8]) -> Result<");
        b.push_str(&view);
        b.push_str("<'_>, __core::DecodeError> {\n        ");
        b.push_str(&view);
        b.push_str("::decode_with_limits(bytes, &__core::DecoderLimits::default())\n    }\n}\n");
        self.out.push_str(&b);
    }

    /// Builds the match arms for owned (`view=false`) or view decoding.
    fn decode_arms(
        &self,
        scope: &[String],
        msg: &ir::MessageIr,
        parent_flat: &str,
        view: bool,
    ) -> String {
        let mut arms = String::new();
        for f in &msg.fields {
            arms.push_str(&self.field_decode_arm(scope, f, view));
        }
        for o in &msg.oneofs {
            for mf in &o.fields {
                arms.push_str(&self.oneof_decode_arm(scope, parent_flat, o, mf, view));
            }
        }
        arms
    }

    /// Match arms decoding one regular field.
    fn field_decode_arm(&self, scope: &[String], f: &ir::FieldIr, view: bool) -> String {
        use ir::FieldLabelIr;
        let target = "out_msg";
        let fname = naming::field_ident(&f.name);
        let dec = |scalar: &str| {
            if view {
                expr::dec_view(scalar, "&field.value", "limits")
            } else {
                expr::dec_owned(scalar, "&field.value", "limits")
            }
        };
        match &f.label {
            FieldLabelIr::Singular(t) => {
                let kind = self.resolve_ref(scope, &t.path).1;
                match kind {
                    TypeKind::Scalar(info) => {
                        let assign = match f.presence {
                            ir::Presence::Explicit => format!("{target}.{fname} = Some({});", "VALUE"),
                            ir::Presence::Implicit => format!("{target}.{fname} = VALUE;"),
                        };
                        let assign = assign.replace(
                            "VALUE",
                            &format!("{}?", dec(t.path[0].as_str())),
                        );
                        format!(
"                ({}, {}) => {{\n                    {}\n                }}\n",
                            f.id,
                            class_name(info.class),
                            assign
                        )
                    }
                    TypeKind::Enum { open } => {
                        let ety = self.resolve_ref(scope, &t.path).0;
                        let val = if open {
                            format!("{ety}::from_i32(n)")
                        } else {
                            format!("{ety}::from_i32(n)?")
                        };
                        let assign = if f.presence == ir::Presence::Explicit {
                            format!("{target}.{fname} = Some({val});")
                        } else {
                            format!("{target}.{fname} = {val};")
                        };
                        format!(
"                ({}, {}) => {{\n                    let n = __support::wire_i32(&field.value)?;\n                    {}\n                }}\n",
                            f.id,
                            class_name(crate::WireClass::Varint),
                            assign
                        )
                    }
                      TypeKind::Message => {
                          let ty = if view {
                              self.view_type(scope, &t.path)
                          } else {
                              self.owned_type(scope, &t.path)
                          };
                          let method = if view {
                              turbo_call(&ty, "decode_inner")
                          } else {
                              format!("{ty}::decode_inner")
                          };
                          let bytes_dec = if view {
                              "__scalar::decode_bytes_borrowed(&field.value)"
                          } else {
                              "__scalar::decode_bytes(&field.value)"
                          };
                          format!(
   "                ({}, {}) => {{\n                    let sub = {bytes_dec}?;\n                    {target}.{fname} = Some({method}(sub, limits, depth + 1)?);\n                }}\n",
                              f.id,
                              class_name(crate::WireClass::Len),
                              bytes_dec = bytes_dec,
                          )
                      }
                }
            }
            FieldLabelIr::Repeated(t) => {
                self.repeated_decode_arms(scope, f, t.path[0].as_str(), &fname, view)
            }
            FieldLabelIr::Map { key, value } => {
                self.map_decode_arm(scope, f, key.path[0].as_str(), value, &fname, view)
            }
        }
    }

    /// Conversion snippet from wire i32 `n` into an enum assignment.
    #[allow(dead_code)]
    fn enum_conv(
        _explicit: bool,
        _open: bool,
        _ty: &str,
    ) -> String {
        String::new()
    }

    /// Match arms for repeated fields (packed AND unpacked accepted).
    fn repeated_decode_arms(
        &self,
        scope: &[String],
        f: &ir::FieldIr,
        scalar: &str,
        fname: &str,
        view: bool,
    ) -> String {
        let id = f.id;
        let kind = self.resolve_ref(scope, f.label.unwrap_type().path.as_slice()).1;
        match kind {
            TypeKind::Scalar(info) => {
                let (single_reader, packed_reader) = match info.pack {
                    PackKind::Varint => (
                        "__scalar::decode_uint(&field.value)?".to_string(),
                        "__scalar::decode_packed_varints(&field.value, limits)?".to_string(),
                    ),
                    PackKind::Fixed32 => (
                        "__scalar::decode_fixed32(&field.value)?".to_string(),
                        "__scalar::decode_packed_fixed32(&field.value, limits)?".to_string(),
                    ),
                    PackKind::Fixed64 => (
                        "__scalar::decode_fixed64(&field.value)?".to_string(),
                        "__scalar::decode_packed_fixed64(&field.value, limits)?".to_string(),
                    ),
                    PackKind::NotPackable => {
                        // string / bytes
                        let dec = if view {
                            expr::dec_view(scalar, "&field.value", "limits")
                        } else {
                            expr::dec_owned(scalar, "&field.value", "limits")
                        };
                        return format!(
"                ({}, {}) => {{\n                    out_msg.{fname}.push({}?);\n                }}\n",
                            id,
                            class_name(crate::WireClass::Len),
                            dec
                        );
                    }
                };
                let from = expr::from_wire_word(scalar, "x");
                format!(
"                ({id}, {cls}) => {{\n                    let x = {single_reader};\n                    out_msg.{fname}.push({from});\n                }}\n                ({id}, {len}) => {{\n                    let words = {packed_reader};\n                    out_msg.{fname}.extend(words.into_iter().map(|x| {from}));\n                    limits.check_repeated_entries(out_msg.{fname}.len())?;\n                }}\n",
                    cls = class_name(info.class),
                    len = class_name(crate::WireClass::Len),
                )
            }
            TypeKind::Enum { open } => {
                let ety = self.resolve_ref(scope, f.label.unwrap_type().path.as_slice()).0;
                if open {
                    format!(
"                ({id}, {varint}) => {{\n                    let n = __support::wire_i32(&field.value)?;\n                    out_msg.{fname}.push({ety}::from_i32(n));\n                }}\n                ({id}, {len}) => {{\n                    let words = __scalar::decode_packed_varints(&field.value, limits)?;\n                    out_msg.{fname}.extend(words.into_iter().map(|x| {ety}::from_i32(x as i32)));\n                    limits.check_repeated_entries(out_msg.{fname}.len())?;\n                }}\n",
                        varint = class_name(crate::WireClass::Varint),
                        len = class_name(crate::WireClass::Len),
                    )
                } else {
                    format!(
"                ({id}, {varint}) => {{\n                    let n = __support::wire_i32(&field.value)?;\n                    out_msg.{fname}.push({ety}::from_i32(n)?);\n                }}\n                ({id}, {len}) => {{\n                    let words = __scalar::decode_packed_varints(&field.value, limits)?;\n                    for x in words {{\n                        out_msg.{fname}.push({ety}::from_i32(x as i32)?);\n                    }}\n                    limits.check_repeated_entries(out_msg.{fname}.len())?;\n                }}\n",
                        varint = class_name(crate::WireClass::Varint),
                        len = class_name(crate::WireClass::Len),
                    )
                }
            }
               TypeKind::Message => {
                   let ty = if view {
                       self.view_type(scope, f.label.unwrap_type().path.as_slice())
                   } else {
                       self.owned_type(scope, f.label.unwrap_type().path.as_slice())
                   };
                   let method = if view {
                       turbo_call(&ty, "decode_inner")
                   } else {
                       format!("{ty}::decode_inner")
                   };
                   let bytes_dec = if view {
                       "__scalar::decode_bytes_borrowed(&field.value)"
                   } else {
                       "__scalar::decode_bytes(&field.value)"
                   };
                   format!(
   "                ({id}, {len}) => {{\n                    let sub = {bytes_dec}?;\n                    out_msg.{fname}.push({method}(sub, limits, depth + 1)?);\n                }}\n",
                       len = class_name(crate::WireClass::Len),
                       bytes_dec = bytes_dec,
                   )
               }
        }
    }

    /// Match arm for map fields (repeated synthetic entry messages).
    #[allow(clippy::too_many_arguments)]
    fn map_decode_arm(
        &self,
        scope: &[String],
        f: &ir::FieldIr,
        key_scalar: &str,
        value: &ir::TypeRefIr,
        fname: &str,
        view: bool,
    ) -> String {
        let kinfo = scalar_info(key_scalar).expect("map keys must be scalar");
        let kdec = if view {
            expr::dec_view(key_scalar, "&ef.value", "limits")
        } else {
            expr::dec_owned(key_scalar, "&ef.value", "limits")
        };
        let vkind = self.resolve_ref(scope, &value.path).1;
        let vt = if view {
            self.view_type(scope, &value.path)
        } else {
            self.owned_type(scope, &value.path)
        };
        let vclass = model::Model::wire_class(vkind);
        let vexpr = match vkind {
            TypeKind::Scalar(_) => {
                let d = if view {
                    expr::dec_view(&value.path[0], "&ef.value", "limits")
                } else {
                    expr::dec_owned(&value.path[0], "&ef.value", "limits")
                };
                format!("{d}?")
            }
            TypeKind::Enum { open } => {
                let ety = self.resolve_ref(scope, &value.path).0;
                if open {
                    format!("{ety}::from_i32(__support::wire_i32(&ef.value)?)")
                } else {
                    format!("{ety}::from_i32(__support::wire_i32(&ef.value)?)?")
                }
            }
              TypeKind::Message => {
                  let ty = if view {
                      self.view_type(scope, &value.path)
                  } else {
                      self.owned_type(scope, &value.path)
                  };
                  let method = if view {
                      turbo_call(&ty, "decode_inner")
                  } else {
                      format!("{ty}::decode_inner")
                  };
                  let bytes_dec = if view {
                      "__scalar::decode_bytes_borrowed(&ef.value)"
                  } else {
                      "__scalar::decode_bytes(&ef.value)"
                  };
                  format!(
                      "{method}({bytes_dec}, limits, depth + 1)?",
                      bytes_dec = bytes_dec,
                  )
              }
        };
        let kt = if view {
            expr::view_rust_type(key_scalar).to_string()
        } else {
            self.owned_type(&[], &[key_scalar.to_string()])
        };
        format!(
            r#"                ({id}, {len}) => {{
                     let entry_bytes = __scalar::decode_bytes_borrowed(&field.value)?;
                     let entry = __core::RawMessage::decode(
                         entry_bytes,
                         limits,
                         __core::UnknownFieldPolicy::Preserve,
                     )?;
                     let mut k: Option<{kt}> = None;
                     let mut v: Option<{vt}> = None;
                     for ef in &entry.fields {{
                         match (ef.field_id, ef.wire_class) {{
                             (1, {kclass}) => {{
                                 k = Some({kdec}?);
                             }}
                             (2, {vclass}) => {{
                                 v = Some({vexpr});
                             }}
                             _ => {{}}
                         }}
                     }}
                      match (k, v) {{
                          (Some(k), Some(v)) => {{
                              out_msg.{fname}.{map_push}({map_args});
                          }}
                          _ => return Err(__core::DecodeError::MalformedMapEntry),
                      }}
                      limits.check_map_entries(out_msg.{fname}.len())?;
                  }}
                "#,
            id = f.id,
            len = class_name(crate::WireClass::Len),
            kt = kt,
            vt = vt,
            kclass = class_name(kinfo.class),
            vclass = class_name(vclass.unwrap_or(crate::WireClass::Len)),
            map_push = if view { "push" } else { "insert" },
            map_args = if view { "(k, v)" } else { "k, v" },
        )
    }

    /// Match arm for a oneof member; each occurrence replaces the previous
    /// value (spec \u{a7}9.8 last-wins).
    #[allow(clippy::too_many_arguments)]
    fn oneof_decode_arm(
        &self,
        scope: &[String],
        parent_flat: &str,
        o: &ir::OneofIr,
        mf: &ir::FieldIr,
        view: bool,
    ) -> String {
        let t = mf.label.unwrap_type();
        let kind = self.resolve_ref(scope, &t.path).1;
        let oname = naming::field_ident(&o.name);
        let ty_name = if view {
            let base = format!("{}{}View", parent_flat, naming::pascal(&o.name));
            if base.contains('<') {
                let idx = base.find('<').unwrap();
                let name = &base[..idx];
                let lt = &base[idx + 1..base.len() - 1];
                format!("{name}::<{lt}>")
            } else {
                base
            }
        } else {
            format!("{}{}", parent_flat, naming::pascal(&o.name))
        };
        let variant = naming::sanitize_ident(&naming::pascal(&mf.name));
        match kind {
            TypeKind::Scalar(info) => {
                let dec = if view {
                    expr::dec_view(t.path[0].as_str(), "&field.value", "limits")
                } else {
                    expr::dec_owned(t.path[0].as_str(), "&field.value", "limits")
                };
                format!(
"                ({}, {}) => {{\n                    out_msg.{oname} = Some({ty_name}::{variant}({dec}?));\n                }}\n",
                    mf.id,
                    class_name(info.class),
                )
            }
            TypeKind::Enum { open } => {
                let ety = self.resolve_ref(scope, &t.path).0;
                let conv = if open {
                    format!("{ety}::from_i32(n)")
                } else {
                    format!("{ety}::from_i32(n)?")
                };
                format!(
"                ({}, {}) => {{\n                    let n = __support::wire_i32(&field.value)?;\n                    out_msg.{oname} = Some({ty_name}::{variant}({conv}));\n                }}\n",
                    mf.id,
                    class_name(crate::WireClass::Varint),
                )
            }
             TypeKind::Message => {
                 let mty = if view {
                     self.view_type(scope, &t.path)
                 } else {
                     self.owned_type(scope, &t.path)
                 };
                 let method = if view {
                     turbo_call(&mty, "decode_inner")
                 } else {
                     format!("{mty}::decode_inner")
                 };
                 let bytes_dec = if view {
                     "__scalar::decode_bytes_borrowed(&field.value)"
                 } else {
                     "__scalar::decode_bytes(&field.value)"
                 };
                 format!(
   "                ({}, {}) => {{\n                    let sub = {bytes_dec}?;\n                    out_msg.{oname} = Some({ty_name}::{variant}({method}(sub, limits, depth + 1)?));\n                }}\n",
                     mf.id,
                     class_name(crate::WireClass::Len),
                     bytes_dec = bytes_dec,
                 )
             }
        }
    }

    /// Emits the borrowed view struct and its decoder.
    fn emit_view(&mut self, scope: &[String], msg: &ir::MessageIr, ctx: &MsgCtx) {
        let flat = format!("{}View", ctx.flat);
        let lt = "<'a>";
        let mut s = String::new();
        s.push_str(&format!(
            "\n/// Borrowed view over `{}` bytes (spec §11.2): strings/bytes borrow,\n/// numerics copy. Unknown fields are dropped here (use owned decoding to\n/// preserve them).\n#[derive(Debug, Clone, PartialEq)]\npub struct {flat}{lt} {{\n",
            ctx.flat
        ));
        s.push_str(&format!("    __raw: __core::BorrowedMessage{lt},\n"));
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            let ty = self.view_field_type(scope, f);
            s.push_str(&format!("    pub {fname}: {ty},\n"));
        }
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            let oty = format!("{}{}View{lt}", ctx.flat, naming::pascal(&o.name));
            s.push_str(&format!("    pub {oname}: Option<{oty}>,\n"));
        }
        s.push_str("}\n");

        // Oneof view enum.
        for o in &msg.oneofs {
            let oty = format!("{}{}View", ctx.flat, naming::pascal(&o.name));
            s.push_str(&format!(
"\n/// Borrowed oneof view for `{}`.\n#[derive(Debug, Clone, PartialEq)]\npub enum {oty}{lt} {{\n",
                o.name
            ));
            for mf in &o.fields {
                let variant = naming::sanitize_ident(&naming::pascal(&mf.name));
                let t = mf.label.unwrap_type();
                let vty = self.view_type(scope, &t.path);
                s.push_str(&format!("    /// Field id {}.\n    {variant}({vty}),\n", mf.id));
            }
            s.push_str("}\n");
        }

        // Decoder impl.
        let known = ctx
            .known_ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let bytes_lt = "'a";
        s.push_str(&format!(
            r#"impl{lt} {flat}{lt} {{
    const KNOWN_IDS: &'static [u32] = &[{known}];

    fn decode_inner(
        bytes: &{bytes_lt} [u8],
        limits: &__core::DecoderLimits,
        depth: usize,
    ) -> Result<Self, __core::DecodeError> {{
        limits.check_depth(depth)?;
        let raw = __core::BorrowedMessage::decode_borrowed_filtered(
            bytes,
            limits,
            __core::UnknownFieldPolicy::Discard,
            &|id| Self::KNOWN_IDS.contains(&id),
        )?;
        let mut out_msg = Self {{
            __raw: raw,
{inits}        }};
        for field in &out_msg.__raw.fields {{
            match (field.field_id, field.wire_class) {{
{arms}                _ => {{
                    return Err(__core::DecodeError::WireClassMismatch {{
                        field_id: field.field_id,
                    }});
                }}
            }}
        }}
        Ok(out_msg)
    }}

    /// Decodes a view with explicit resource limits.
    pub fn decode_with_limits(
        bytes: &{bytes_lt} [u8],
        limits: &__core::DecoderLimits,
    ) -> Result<Self, __core::DecodeError> {{
        Self::decode_inner(bytes, limits, 1)
    }}
}}
"#,
            inits = self.view_inits(scope, msg),
            arms = self.decode_arms(scope, msg, &ctx.flat, true),
        ));
        self.out.push_str(&s);
    }

    /// Rust type for a view struct field.
    fn view_field_type(&self, scope: &[String], f: &ir::FieldIr) -> String {
        match &f.label {
            ir::FieldLabelIr::Singular(t) => {
                let kind = self.resolve_ref(scope, &t.path).1;
                match kind {
                    TypeKind::Message => format!("Option<{}>", self.view_type(scope, &t.path)),
                    _ => {
                        if model::is_scalar_path(&t.path)
                            && matches!(t.path[0].as_str(), "string" | "bytes")
                        {
                            return match f.presence {
                                ir::Presence::Explicit => {
                                    format!("Option<{}>", expr::view_rust_type(&t.path[0]))
                                }
                                ir::Presence::Implicit => {
                                    expr::view_rust_type(&t.path[0]).to_string()
                                }
                            };
                        }
                        match f.presence {
                            ir::Presence::Explicit => {
                                format!("Option<{}>", self.owned_type(scope, &t.path))
                            }
                            ir::Presence::Implicit => self.owned_type(scope, &t.path),
                        }
                    }
                }
            }
            ir::FieldLabelIr::Repeated(t) => {
                format!("Vec<{}>", self.view_type(scope, &t.path))
            }
            ir::FieldLabelIr::Map { key, value } => format!(
                "Vec<({}, {})>",
                self.view_type(scope, &key.path),
                self.view_type(scope, &value.path)
            ),
        }
    }

    /// Explicit initializer expressions for view fields.
    fn view_inits(&self, scope: &[String], msg: &ir::MessageIr) -> String {
        let mut out = String::new();
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            let init = match &f.label {
                ir::FieldLabelIr::Singular(t) => {
                    if model::is_scalar_path(&t.path)
                        && matches!(t.path[0].as_str(), "string" | "bytes")
                    {
                        match f.presence {
                            ir::Presence::Explicit => String::from("None"),
                            ir::Presence::Implicit => {
                                if t.path[0] == "string" {
                                    String::from("\"\"")
                                } else {
                                    String::from("&[][..]")
                                }
                            }
                        }
                    } else if self.resolve_ref(scope, &t.path).1 == TypeKind::Message
                        || f.presence == ir::Presence::Explicit
                    {
                        String::from("None")
                    } else if let TypeKind::Enum { .. } = self.resolve_ref(scope, &t.path).1 {
                        format!("{}::default()", self.owned_type(scope, &t.path))
                    } else {
                        scalar_zero(t.path[0].as_str()).to_string()
                    }
                }
                ir::FieldLabelIr::Repeated(_) | ir::FieldLabelIr::Map { .. } => String::from("Vec::new()"),
            };
            out.push_str(&format!("                {}: {},\n", fname, init));
        }
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            out.push_str(&format!("                {oname}: None,\n"));
        }
        out
    }

    /// Emits JSON conversion methods (spec \u{a7}14.2).
    fn emit_json(&mut self, scope: &[String], msg: &ir::MessageIr, ctx: &MsgCtx) {
        let flat = &ctx.flat;
        let mut s = String::new();
        s.push_str(&format!("impl {flat} {{\n"));
        s.push_str(
"    /// Converts to a JSON value (spec \u{a7}14.2): original field names,\n    /// 64-bit ints as strings, bytes as base64, defaults omitted.\n    pub fn to_json_value(&self) -> Result<__json::Value, __json::JsonError> {\n        let mut obj = __json::json::Map::new();\n",
        );
        self.push_json_inserts(scope, msg, ctx, &mut s);
        self.push_json_oneof_inserts(scope, msg, ctx, &mut s);
        s.push_str(
"        Ok(__json::Value::Object(obj))\n    }\n\n    /// Serializes to a JSON string.\n    pub fn to_json(&self) -> Result<String, __json::JsonError> {\n        let v = self.to_json_value()?;\n        __json::json::to_string(&v).map_err(Into::into)\n    }\n",
        );
        s.push_str(&self.json_from_body(scope, msg, ctx));
        s.push_str("}\n");
        self.out.push_str(&s);
    }

    /// `obj.insert(...)` lines for regular fields.
    fn push_json_inserts(
        &self,
        scope: &[String],
        msg: &ir::MessageIr,
        _ctx: &MsgCtx,
        s: &mut String,
    ) {
        use ir::FieldLabelIr;
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            let name_lit = format!("{:?}", f.name);
            match &f.label {
                FieldLabelIr::Singular(t) => {
                    let kind = self.resolve_ref(scope, &t.path).1;
                    match kind {
                        TypeKind::Scalar(_) => {
                            let jt = expr::json_to(t.path[0].as_str(), "v");
                            match f.presence {
                                ir::Presence::Explicit => s.push_str(&format!(
"        if let Some(v) = &self.{fname} {{\n            obj.insert({name_lit}.to_string(), {jt});\n        }}\n"
                                )),
                                ir::Presence::Implicit => {
                                    let cond = Self::skip_cond(t.path[0].as_str(), "v");
                                    s.push_str(&format!(
"        let v = &self.{fname};\n        if {cond} {{\n            obj.insert({name_lit}.to_string(), {jt});\n        }}\n"
                                    ));
                                }
                            }
                        }
                        TypeKind::Enum { .. } => {
                            let ety = self.resolve_ref(scope, &t.path).0;
                            match f.presence {
                                ir::Presence::Explicit => s.push_str(&format!(
"        if let Some(v) = &self.{fname} {{\n            obj.insert({name_lit}.to_string(), {ety}::json_name(v));\n        }}\n"
                                )),
                                ir::Presence::Implicit => s.push_str(&format!(
"        if self.{fname}.to_i32() != 0 {{\n            obj.insert({name_lit}.to_string(), {ety}::json_name(&self.{fname}));\n        }}\n"
                                )),
                            }
                        }
                        TypeKind::Message => s.push_str(&format!(
"        if let Some(v) = &self.{fname} {{\n            obj.insert({name_lit}.to_string(), v.to_json_value()?);\n        }}\n"
                        )),
                    }
                }
                FieldLabelIr::Repeated(t) => {
                    let mapper = self.json_mapper(scope, t.path[0].as_str(), t);
                    s.push_str(&format!(
"        if !self.{fname}.is_empty() {{\n            let arr: Vec<__json::Value> = self.{fname}.iter().map(|v| {mapper}).collect();\n            obj.insert({name_lit}.to_string(), __json::Value::Array(arr));\n        }}\n"
                    ));
                }
                FieldLabelIr::Map { key, value } => {
                    let vmapper = self.json_mapper(scope, value.path[0].as_str(), value);
                    let kstr = key_to_string(key.path[0].as_str(), "k");
                    s.push_str(&format!(
"        if !self.{fname}.is_empty() {{\n            let mobj: __json::json::Map<String, __json::Value> = self\n                .{fname}\n                .iter()\n                .map(|(k, v)| ({kstr}, {vmapper}))\n                .collect();\n            obj.insert({name_lit}.to_string(), __json::Value::Object(mobj));\n        }}\n"
                    ));
                }
            }
        }
    }

    /// Mapper expression turning a borrowed element into a JSON value.
    fn json_mapper(&self, scope: &[String], scalar: &str, t: &ir::TypeRefIr) -> String {
        match self.resolve_ref(scope, &t.path).1 {
            TypeKind::Scalar(_) => expr::json_to(scalar, "v"),
            TypeKind::Enum { .. } => format!("{}::json_name(v)", self.resolve_ref(scope, &t.path).0),
            TypeKind::Message => "v.to_json_value()?".to_string(),
        }
    }

    /// Oneof JSON inserts (flattened under member names).
    fn push_json_oneof_inserts(
        &self,
        scope: &[String],
        msg: &ir::MessageIr,
        ctx: &MsgCtx,
        s: &mut String,
    ) {
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            let oty = format!("{}{}", ctx.flat, naming::pascal(&o.name));
            s.push_str(&format!("        match &self.{oname} {{\n"));
            for mf in &o.fields {
                let variant = naming::sanitize_ident(&naming::pascal(&mf.name));
                let t = mf.label.unwrap_type();
                let val_expr = self.json_mapper(scope, t.path[0].as_str(), t);
                s.push_str(&format!(
"            Some({oty}::{variant}(v)) => {{\n                obj.insert({:?}.to_string(), {val_expr});\n            }}\n",
                    mf.name
                ));
            }
            s.push_str("            None => {}\n        }\n");
        }
    }

    /// Emits `from_json_value` / `from_json` bodies.
    fn json_from_body(&self, scope: &[String], msg: &ir::MessageIr, ctx: &MsgCtx) -> String {
        use ir::FieldLabelIr;
        let mut b = String::new();
        b.push_str(
"    /// Parses from a parsed JSON value.\n    pub fn from_json_value(v: &__json::Value) -> Result<Self, __json::JsonError> {\n        let obj = v\n            .as_object()\n            .ok_or(__json::JsonError::TypeMismatch { expected: \"object\" })?;\n        let mut out_msg = Self::default();\n",
        );
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            let names = format!("&[{:?}, {:?}]", f.name, naming::lower_camel(&f.name));
            match &f.label {
                FieldLabelIr::Singular(t) => {
                    let kind = self.resolve_ref(scope, &t.path).1;
                    let wrap = |exprs: String| match f.presence {
                        ir::Presence::Explicit => format!("out_msg.{fname} = Some({exprs});"),
                        ir::Presence::Implicit => format!("out_msg.{fname} = {exprs};"),
                    };
                    match kind {
                        TypeKind::Scalar(_) => {
                            let jf = expr::json_from(t.path[0].as_str(), "jv");
                            b.push_str(&format!(
"        if let Some(jv) = __json::get_field(obj, {names}) {{\n            {}\n        }}\n",
                                wrap(format!("{jf}?"))
                            ));
                        }
                        TypeKind::Enum { .. } => {
                            let ety = self.resolve_ref(scope, &t.path).0;
                            b.push_str(&format!(
"        if let Some(jv) = __json::get_field(obj, {names}) {{\n            {}\n        }}\n",
                                wrap(format!("{ety}::from_json(jv)?"))
                            ));
                        }
                        TypeKind::Message => {
                            let ty = self.owned_type(scope, &t.path);
                            b.push_str(&format!(
"        if let Some(jv) = __json::get_field(obj, {names}) {{\n            out_msg.{fname} = Some({ty}::from_json_value(jv)?);\n        }}\n"
                            ));
                        }
                    }
                }
                FieldLabelIr::Repeated(t) => {
                    let jf = self.json_from_expr(scope, t);
                    b.push_str(&format!(
"        if let Some(jv) = __json::get_field(obj, {names}) {{\n            if let __json::Value::Array(items) = jv {{\n                for item in items {{\n                    out_msg.{fname}.push({jf}?);\n                }}\n            }} else {{\n                return Err(__json::JsonError::TypeMismatch {{ expected: \"array\" }});\n            }}\n        }}\n"
                    ));
                }
                FieldLabelIr::Map { key, value } => {
                    let kparse = key_from_string(key.path[0].as_str());
                    let vjf = self.json_from_expr(scope, value);
                    b.push_str(&format!(
"        if let Some(jv) = __json::get_field(obj, {names}) {{\n            if let __json::Value::Object(m) = jv {{\n                for (ks, item) in m {{\n                    let k = {kparse}?;\n                    let v = {vjf}?;\n                    out_msg.{fname}.insert(k, v);\n                }}\n            }} else {{\n                return Err(__json::JsonError::TypeMismatch {{ expected: \"object\" }});\n            }}\n        }}\n"
                    ));
                }
            }
        }
        b.push_str(&self.json_oneof_pulls(scope, msg, ctx));
        b.push_str(
"        Ok(out_msg)\n    }\n\n    /// Parses from a JSON string.\n    pub fn from_json(json: &str) -> Result<Self, __json::JsonError> {\n        let v: __json::Value = __json::json::from_str(json)?;\n        Self::from_json_value(&v)\n    }\n",
        );
        b
    }

    /// Oneof JSON pulls (flattened member names; later members win).
    fn json_oneof_pulls(
        &self,
        scope: &[String],
        msg: &ir::MessageIr,
        ctx: &MsgCtx,
    ) -> String {
        let mut b = String::new();
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            let ty_name = format!("{}{}", ctx.flat, naming::pascal(&o.name));
            for mf in &o.fields {
                let names = format!("&[{:?}, {:?}]", mf.name, naming::lower_camel(&mf.name));
                let t = mf.label.unwrap_type();
                let kind = self.resolve_ref(scope, &t.path).1;
                let variant = naming::sanitize_ident(&naming::pascal(&mf.name));
                let val_expr = match kind {
                    TypeKind::Scalar(_) => {
                        format!("{}?", expr::json_from(t.path[0].as_str(), "jv"))
                    }
                    TypeKind::Enum { .. } => format!(
                        "{}::from_json(jv)?",
                        self.resolve_ref(scope, &t.path).0
                    ),
                    TypeKind::Message => format!(
                        "{}::from_json_value(jv)?",
                        self.resolve_ref(scope, &t.path).0
                    ),
                };
                b.push_str(&format!(
"        if let Some(jv) = __json::get_field(obj, {names}) {{\n            out_msg.{oname} = Some({ty_name}::{variant}({val_expr}));\n        }}\n"
                ));
            }
        }
        b
    }

    /// Element mapper used by repeated/map JSON parsing.
    fn json_from_expr(&self, scope: &[String], t: &ir::TypeRefIr) -> String {
        match self.resolve_ref(scope, &t.path).1 {
            TypeKind::Scalar(_) => expr::json_from(t.path[0].as_str(), "item"),
            TypeKind::Enum { .. } => format!("{}::from_json(item)", self.resolve_ref(scope, &t.path).0),
            TypeKind::Message => format!(
                "{}::from_json_value(item)",
                self.resolve_ref(scope, &t.path).0
            ),
        }
    }

    /// Emits opt-in builders with annotation validation (spec \u{a7}12.3).
    fn emit_builder(&mut self, scope: &[String], msg: &ir::MessageIr, ctx: &MsgCtx) {
        use ir::FieldLabelIr;
        let flat = &ctx.flat;
        let bty = format!("{}Builder", ctx.flat);
        let mut s = String::new();

        s.push_str(&format!(
"impl {flat} {{\n    /// Returns a new builder.\n    pub fn builder() -> {bty} {{\n        {bty}::default()\n    }}\n}}\n"
        ));
        s.push_str(&format!(
"\n/// Builder for `{flat}` with annotation validation.\n#[derive(Debug, Clone, Default)]\npub struct {bty} {{\n"
        ));
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            s.push_str(&format!("    {fname}: {},\n", self.struct_field_type(scope, f)));
        }
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            let oty = format!("{}{}", ctx.flat, naming::pascal(&o.name));
            s.push_str(&format!(
"    {oname}: Option<{oty}>,\n"
            ));
        }
        s.push_str("}\n");

        s.push_str(&format!("impl {bty} {{\n"));
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            match &f.label {
                FieldLabelIr::Singular(t) => match self.resolve_ref(scope, &t.path).1 {
                    TypeKind::Message => {
                        let ty = self.owned_type(scope, &t.path);
                        s.push_str(&format!(
"    /// Sets `{fname}`.\n    pub fn {fname}(mut self, v: {ty}) -> Self {{\n        self.{fname} = Some(v);\n        self\n    }}\n"
                        ));
                    }
                    TypeKind::Enum { .. } => {
                        let ety = self.owned_type(scope, &t.path);
                        if f.presence == ir::Presence::Explicit {
                            s.push_str(&format!(
"    pub fn {fname}(mut self, v: {ety}) -> Self {{\n        self.{fname} = Some(v);\n        self\n    }}\n"
                            ));
                        } else {
                            s.push_str(&format!(
"    pub fn {fname}(mut self, v: {ety}) -> Self {{\n        self.{fname} = v;\n        self\n    }}\n"
                            ));
                        }
                    }
                    TypeKind::Scalar(info) => {
                        if matches!(t.path[0].as_str(), "string" | "bytes") && f.presence == ir::Presence::Explicit {
                            s.push_str(&format!(
"    pub fn {fname}(mut self, v: impl Into<{}>) -> Self {{\n        self.{fname} = Some(v.into());\n        self\n    }}\n",
                                info.rust
                            ));
                        } else if matches!(t.path[0].as_str(), "string" | "bytes") {
                            s.push_str(&format!(
"    pub fn {fname}(mut self, v: impl Into<{}>) -> Self {{\n        self.{fname} = v.into();\n        self\n    }}\n",
                                info.rust
                            ));
                        } else if f.presence == ir::Presence::Explicit {
                            s.push_str(&format!(
"    pub fn {fname}(mut self, v: {}) -> Self {{\n        self.{fname} = Some(v);\n        self\n    }}\n",
                                info.rust
                            ));
                        } else {
                            s.push_str(&format!(
"    pub fn {fname}(mut self, v: {}) -> Self {{\n        self.{fname} = v;\n        self\n    }}\n",
                                info.rust
                            ));
                        }
                    }
                },
                FieldLabelIr::Repeated(t) => {
                    let ity = self.owned_type(scope, &t.path);
                    s.push_str(&format!(
"    pub fn {fname}(mut self, v: impl IntoIterator<Item = {ity}>) -> Self {{\n        self.{fname} = v.into_iter().collect();\n        self\n    }}\n"
                    ));
                }
                FieldLabelIr::Map { key, value } => {
                    let kt = self.owned_type(scope, &key.path);
                    let vt = self.owned_type(scope, &value.path);
                    s.push_str(&format!(
"    pub fn {fname}(mut self, v: impl IntoIterator<Item = ({kt}, {vt})>) -> Self {{\n        self.{fname} = v.into_iter().collect();\n        self\n    }}\n"
                    ));
                }
            }
        }
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            let oty = format!("{}{}", ctx.flat, naming::pascal(&o.name));
            s.push_str(&format!(
"    pub fn {oname}(mut self, v: {oty}) -> Self {{\n        self.{oname} = Some(v);\n        self\n    }}\n"
            ));
        }
        // build(): validations + construction.
        s.push_str(&format!(
"    /// Validates annotations and builds the message.\n    pub fn build(self) -> Result<{flat}, BuildError> {{\n"
        ));
        for f in &msg.fields {
            s.push_str(&self.builder_validation(f));
        }
        s.push_str(&format!("        Ok({flat} {{\n"));
        for f in &msg.fields {
            let fname = naming::field_ident(&f.name);
            s.push_str(&format!("            {fname}: self.{fname},\n"));
        }
        for o in &msg.oneofs {
            let oname = naming::field_ident(&o.name);
            s.push_str(&format!("            {oname}: self.{oname},\n"));
        }
        s.push_str(
"            unknown_fields: __core::RawMessage::new(),\n        })\n    }\n}\n",
        );
        self.out.push_str(&s);
    }

    /// Validation statements for one field inside `build()`.
    fn builder_validation(&self, f: &ir::FieldIr) -> String {
        let fname = naming::field_ident(&f.name);
        let name_lit = format!("{:?}", f.name);
        let mut out = String::new();
        if let Some(max) = anno_int(f, "max_len") {
            let check = if f.presence == ir::Presence::Explicit {
                format!(
"        if let Some(v) = &self.{fname} {{\n            if v.len() > {max} {{\n                return Err(BuildError::MaxLenExceeded {{ field: {name_lit}, max: {max} }});\n            }}\n        }}\n"
                )
            } else {
                format!(
"        if self.{fname}.len() > {max} {{\n            return Err(BuildError::MaxLenExceeded {{ field: {name_lit}, max: {max} }});\n        }}\n"
                )
            };
            out.push_str(&check);
        }
        if let Some(min) = anno_int(f, "min_len") {
            let check = if f.presence == ir::Presence::Explicit {
                format!(
"        if let Some(v) = &self.{fname} {{\n            if v.len() < {min} {{\n                return Err(BuildError::MinLenViolation {{ field: {name_lit}, min: {min} }});\n            }}\n        }}\n"
                )
            } else {
                format!(
"        if self.{fname}.len() < {min} {{\n            return Err(BuildError::MinLenViolation {{ field: {name_lit}, min: {min} }});\n        }}\n"
                )
            };
            out.push_str(&check);
        }
        if let Some((lo, hi)) = anno_range(f) {
            let check = if f.presence == ir::Presence::Explicit {
                format!(
"        if let Some(v) = &self.{fname} {{\n            if !(({lo})..=({hi})).contains(v) {{\n                return Err(BuildError::OutOfRange {{ field: {name_lit} }});\n            }}\n        }}\n"
                )
            } else {
                format!(
"        if !(({lo})..=({hi})).contains(&self.{fname}) {{\n            return Err(BuildError::OutOfRange {{ field: {name_lit} }});\n        }}\n"
                )
            };
            out.push_str(&check);
        }
        out
    }

}

/// JSON object-key stringifier expression for map keys.
fn key_to_string(key_scalar: &str, k: &str) -> String {
    match key_scalar {
        "string" => format!("{k}.clone()"),
        _ => format!("{k}.to_string()"),
    }
}

/// JSON string -> map key parse expression (`Result<K, JsonError>` on `ks`).
fn key_from_string(key_scalar: &str) -> String {
    match key_scalar {
        "string" => "Ok::<String, __json::JsonError>(ks.clone())".to_string(),
        "bool" => "ks.parse::<bool>().map_err(|_| __json::JsonError::TypeMismatch { expected: \"bool\" })".to_string(),
        "uint64" | "fixed64" => "ks.parse::<u64>().map_err(|_| __json::JsonError::TypeMismatch { expected: \"uint64\" })".to_string(),
        "uint32" | "fixed32" => "ks.parse::<u32>().map_err(|_| __json::JsonError::TypeMismatch { expected: \"uint32\" })".to_string(),
        "int32" | "sint32" => "ks.parse::<i32>().map_err(|_| __json::JsonError::TypeMismatch { expected: \"int32\" })".to_string(),
        _ => "ks.parse::<i64>().map_err(|_| __json::JsonError::TypeMismatch { expected: \"int64\" })".to_string(),
    }
}

/// Zero-value literal for a scalar type (view initializers).
fn scalar_zero(scalar: &str) -> &'static str {
    match scalar {
        "bool" => "false",
        "float32" | "float64" => "0.0",
        "string" => "\"\"",
        "bytes" => "&[][..]",
        _ => "0",
    }
}

/// First integer argument of annotation `@name`, if present.
fn anno_int(f: &ir::FieldIr, name: &str) -> Option<i64> {
    f.annotations
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| a.args.iter().find_map(|arg| match arg {
            ir::AnnotationArgIr::Int(n) => Some(*n),
            _ => None,
        }))
}

/// `(min, max)` from `@range(min, max)` positional integers.
fn anno_range(f: &ir::FieldIr) -> Option<(i64, i64)> {
    let a = f.annotations.iter().find(|a| a.name == "range")?;
    let mut ints = a.args.iter().filter_map(|arg| match arg {
        ir::AnnotationArgIr::Int(n) => Some(*n),
        _ => None,
    });
    Some((ints.next()?, ints.next()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_resolve_through_pipeline() {
        let src = r#"
            package p.v1;
            message Address { 1: street string; }
            message Outer {
                1: id int64;
                14: home Address;
            }
        "#;
        let out = tpt20_compiler::compile(src, None).expect("compiles");
        let m = Model::build(&out.ir);
        println!("names: {:?}", m.names);
        println!("kinds: {:?}", m.kinds);
        println!(
            "resolve Address: {:?}",
            m.resolve(&[], &["Address".to_string()])
        );
        println!(
            "resolve int64: {:?}",
            m.resolve(&[], &["int64".to_string()])
        );
        let opts = CodegenOptions::default();
        let text = generate_module_pub(&out.ir, &opts);
        println!("--- Address-related lines ---");
        for (i, line) in text.lines().enumerate() {
            if line.contains("street")
                || line.contains("home")
                || line.contains("pub id")
                || line.contains("(1,")
            {
                println!("{:4}: {}", i + 1, line);
            }
        }
        assert!(text.contains("pub struct Address"));
    }

    fn generate_module_pub(
        pkg: &tpt20_ir::PackageIr,
        opts: &CodegenOptions,
    ) -> String {
        Emitter::new(pkg, opts).generate()
    }
}


/// Wire-class expression as referenced from generated code.
pub(crate) fn class_name(c: crate::WireClass) -> &'static str {
    match c {
        crate::WireClass::Varint => "__core::WireClass::Varint",
        crate::WireClass::Fixed32 => "__core::WireClass::Fixed32",
        crate::WireClass::Fixed64 => "__core::WireClass::Fixed64",
        crate::WireClass::Len => "__core::WireClass::Len",
    }
}

/// Produces a turbo-path call like `Type::<'a>::method` when `ty` carries a
/// lifetime parameter, falling back to `Type::method` otherwise.
pub(crate) fn turbo_call(ty: &str, method: &str) -> String {
    if let Some(idx) = ty.find('<') {
        let base = &ty[..idx];
        let lt = &ty[idx..];
        let lt_inner = &lt[1..lt.len() - 1];
        format!("{base}::<{lt_inner}>::{method}")
    } else {
        format!("{ty}::{method}")
    }
}

/// Core scalar helper for packed encoding of this pack kind.
pub(crate) fn packed_encode_fn(pack: PackKind) -> &'static str {
    match pack {
        PackKind::Varint => "__scalar::encode_packed_varints",
        PackKind::Fixed32 => "__scalar::encode_packed_fixed32",
        PackKind::Fixed64 => "__scalar::encode_packed_fixed64",
        PackKind::NotPackable => unreachable!(),
    }
}



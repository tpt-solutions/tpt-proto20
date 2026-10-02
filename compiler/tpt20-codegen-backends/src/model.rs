//! A resolved, language-neutral view of a schema, shared by all backends.
//!
//! Type references are resolved (with the same scoping rules as the Rust
//! backend), nested declarations are flattened to `Outer_Inner` names, and
//! every field is classified, so a backend only has to print.

use crate::BackendError;
use std::collections::HashMap;
use tpt20_ir as ir;

/// Scalar field types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scalar {
    Bool,
    Int32,
    Int64,
    UInt32,
    UInt64,
    SInt32,
    SInt64,
    Fixed32,
    Fixed64,
    SFixed32,
    SFixed64,
    Float32,
    Float64,
    String,
    Bytes,
}

/// How a scalar is laid out on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    Varint,
    Fixed32,
    Fixed64,
    Len,
}

impl Scalar {
    fn parse(name: &str) -> Option<Scalar> {
        Some(match name {
            "bool" => Scalar::Bool,
            "int32" => Scalar::Int32,
            "int64" => Scalar::Int64,
            "uint32" => Scalar::UInt32,
            "uint64" => Scalar::UInt64,
            "sint32" => Scalar::SInt32,
            "sint64" => Scalar::SInt64,
            "fixed32" => Scalar::Fixed32,
            "fixed64" => Scalar::Fixed64,
            "sfixed32" => Scalar::SFixed32,
            "sfixed64" => Scalar::SFixed64,
            "float32" => Scalar::Float32,
            "float64" => Scalar::Float64,
            "string" => Scalar::String,
            "bytes" => Scalar::Bytes,
            _ => return None,
        })
    }

    /// Schema spelling (`int32`, `float64`, …).
    pub fn name(self) -> &'static str {
        match self {
            Scalar::Bool => "bool",
            Scalar::Int32 => "int32",
            Scalar::Int64 => "int64",
            Scalar::UInt32 => "uint32",
            Scalar::UInt64 => "uint64",
            Scalar::SInt32 => "sint32",
            Scalar::SInt64 => "sint64",
            Scalar::Fixed32 => "fixed32",
            Scalar::Fixed64 => "fixed64",
            Scalar::SFixed32 => "sfixed32",
            Scalar::SFixed64 => "sfixed64",
            Scalar::Float32 => "float32",
            Scalar::Float64 => "float64",
            Scalar::String => "string",
            Scalar::Bytes => "bytes",
        }
    }

    /// Wire class of a single value.
    pub fn wire(self) -> Wire {
        match self {
            Scalar::Bool
            | Scalar::Int32
            | Scalar::Int64
            | Scalar::UInt32
            | Scalar::UInt64
            | Scalar::SInt32
            | Scalar::SInt64 => Wire::Varint,
            Scalar::Fixed32 | Scalar::SFixed32 | Scalar::Float32 => Wire::Fixed32,
            Scalar::Fixed64 | Scalar::SFixed64 | Scalar::Float64 => Wire::Fixed64,
            Scalar::String | Scalar::Bytes => Wire::Len,
        }
    }

    /// Whether repeated values are packed into one length-delimited field.
    pub fn packable(self) -> bool {
        self.wire() != Wire::Len
    }
}

/// The type of a field (or a map value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Scalar(Scalar),
    /// Index into [`Model::enums`].
    Enum(usize),
    /// Index into [`Model::messages`].
    Message(usize),
}

/// Field cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Card {
    /// Implicit presence: the default value is not emitted.
    Implicit,
    /// Explicit presence: absent is distinguishable from the default.
    Explicit,
    Repeated,
    /// Map with a scalar key (value type is the field's `ty`).
    Map(Scalar),
}

/// One field.
#[derive(Debug, Clone)]
pub struct Field {
    pub id: u32,
    /// Name as written in the schema.
    pub name: String,
    pub ty: Ty,
    pub card: Card,
}

/// A oneof group; members are ordinary fields sharing the group.
#[derive(Debug, Clone)]
pub struct Oneof {
    pub name: String,
    pub fields: Vec<Field>,
}

/// One message.
#[derive(Debug, Clone)]
pub struct Message {
    /// Dotted full name within the package (`Outer.Inner`).
    pub full_name: String,
    /// Flattened identifier (`Outer_Inner`).
    pub flat: String,
    pub fields: Vec<Field>,
    pub oneofs: Vec<Oneof>,
}

impl Message {
    /// All field ids this message knows (regular and oneof members).
    pub fn known_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.fields.iter().map(|f| f.id).collect();
        ids.extend(
            self.oneofs
                .iter()
                .flat_map(|o| o.fields.iter().map(|f| f.id)),
        );
        ids
    }
}

/// An enum value.
#[derive(Debug, Clone)]
pub struct EnumValue {
    pub name: String,
    pub number: i32,
    pub alias: bool,
}

/// One enum.
#[derive(Debug, Clone)]
pub struct Enum {
    pub full_name: String,
    pub flat: String,
    /// Open enums preserve unknown numbers; closed ones reject them.
    pub open: bool,
    pub values: Vec<EnumValue>,
}

/// One RPC method.
#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    /// Index into [`Model::messages`].
    pub request: usize,
    pub response: usize,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

/// One service.
#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    pub methods: Vec<Method>,
}

/// The resolved schema.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// Schema package (`demo.v1`), possibly empty.
    pub package: String,
    pub messages: Vec<Message>,
    pub enums: Vec<Enum>,
    pub services: Vec<Service>,
}

#[derive(Clone, Copy)]
enum Decl {
    Message(usize),
    Enum(usize),
}

impl Model {
    /// Resolves `pkg` into a [`Model`].
    pub fn build(pkg: &ir::PackageIr) -> Result<Model, BackendError> {
        let mut model = Model {
            package: pkg.name.clone().unwrap_or_default(),
            ..Default::default()
        };
        let mut decls: HashMap<String, Decl> = HashMap::new();

        // Pass 1: declare everything so references can point forward.
        fn declare_message(
            m: &ir::MessageIr,
            prefix: &str,
            model: &mut Model,
            decls: &mut HashMap<String, Decl>,
        ) {
            let full = join(prefix, &m.name);
            decls.insert(full.clone(), Decl::Message(model.messages.len()));
            model.messages.push(Message {
                flat: full.replace('.', "_"),
                full_name: full.clone(),
                fields: Vec::new(),
                oneofs: Vec::new(),
            });
            for e in &m.enums {
                declare_enum(e, &full, model, decls);
            }
            for n in &m.messages {
                declare_message(n, &full, model, decls);
            }
        }
        fn declare_enum(
            e: &ir::EnumIr,
            prefix: &str,
            model: &mut Model,
            decls: &mut HashMap<String, Decl>,
        ) {
            let full = join(prefix, &e.name);
            decls.insert(full.clone(), Decl::Enum(model.enums.len()));
            model.enums.push(Enum {
                flat: full.replace('.', "_"),
                full_name: full,
                open: e.open,
                values: e
                    .values
                    .iter()
                    .map(|v| EnumValue {
                        name: v.name.clone(),
                        number: v.number,
                        alias: v.alias,
                    })
                    .collect(),
            });
        }
        for m in &pkg.messages {
            declare_message(m, "", &mut model, &mut decls);
        }
        for e in &pkg.enums {
            declare_enum(e, "", &mut model, &mut decls);
        }

        // Pass 2: fields.
        fn fill_message(
            m: &ir::MessageIr,
            prefix: &str,
            model: &mut Model,
            decls: &HashMap<String, Decl>,
        ) -> Result<(), BackendError> {
            let full = join(prefix, &m.name);
            let Some(Decl::Message(idx)) = decls.get(&full).copied() else {
                return Ok(());
            };
            let fields = m
                .fields
                .iter()
                .map(|f| lower_field(f, &full, decls))
                .collect::<Result<Vec<_>, _>>()?;
            let oneofs = m
                .oneofs
                .iter()
                .map(|o| {
                    Ok(Oneof {
                        name: o.name.clone(),
                        fields: o
                            .fields
                            .iter()
                            .map(|f| {
                                let mut f = lower_field(f, &full, decls)?;
                                f.card = Card::Implicit;
                                Ok(f)
                            })
                            .collect::<Result<Vec<_>, BackendError>>()?,
                    })
                })
                .collect::<Result<Vec<_>, BackendError>>()?;
            model.messages[idx].fields = fields;
            model.messages[idx].oneofs = oneofs;
            for n in &m.messages {
                fill_message(n, &full, model, decls)?;
            }
            Ok(())
        }
        for m in &pkg.messages {
            fill_message(m, "", &mut model, &decls)?;
        }

        for s in &pkg.services {
            let mut methods = Vec::new();
            for m in &s.methods {
                let req = resolve_message(&decls, "", &m.request.path)?;
                let resp = resolve_message(&decls, "", &m.response.path)?;
                methods.push(Method {
                    name: m.name.clone(),
                    request: req,
                    response: resp,
                    client_streaming: m.request_streaming,
                    server_streaming: m.response_streaming,
                });
            }
            model.services.push(Service {
                name: s.name.clone(),
                methods,
            });
        }
        Ok(model)
    }

    /// The message a type refers to.
    pub fn message(&self, ty: Ty) -> Option<&Message> {
        match ty {
            Ty::Message(i) => self.messages.get(i),
            _ => None,
        }
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

fn lookup(decls: &HashMap<String, Decl>, scope: &str, path: &[String]) -> Option<Decl> {
    let rel = path.join(".");
    let mut scope = scope.to_string();
    loop {
        if let Some(d) = decls.get(&join(&scope, &rel)) {
            return Some(*d);
        }
        match scope.rfind('.') {
            Some(i) => scope.truncate(i),
            None if scope.is_empty() => return None,
            None => scope.clear(),
        }
    }
}

fn resolve_message(
    decls: &HashMap<String, Decl>,
    scope: &str,
    path: &[String],
) -> Result<usize, BackendError> {
    match lookup(decls, scope, path) {
        Some(Decl::Message(i)) => Ok(i),
        _ => Err(BackendError::UnresolvedType(path.join("."))),
    }
}

fn resolve_ty(
    decls: &HashMap<String, Decl>,
    scope: &str,
    t: &ir::TypeRefIr,
) -> Result<Ty, BackendError> {
    if t.path.len() == 1 {
        if let Some(s) = Scalar::parse(&t.path[0]) {
            return Ok(Ty::Scalar(s));
        }
    }
    match lookup(decls, scope, &t.path) {
        Some(Decl::Message(i)) => Ok(Ty::Message(i)),
        Some(Decl::Enum(i)) => Ok(Ty::Enum(i)),
        None => Err(BackendError::UnresolvedType(t.path.join("."))),
    }
}

fn lower_field(
    f: &ir::FieldIr,
    scope: &str,
    decls: &HashMap<String, Decl>,
) -> Result<Field, BackendError> {
    let (ty, card) = match &f.label {
        ir::FieldLabelIr::Singular(t) => (
            resolve_ty(decls, scope, t)?,
            if f.presence == ir::Presence::Explicit {
                Card::Explicit
            } else {
                Card::Implicit
            },
        ),
        ir::FieldLabelIr::Repeated(t) => (resolve_ty(decls, scope, t)?, Card::Repeated),
        ir::FieldLabelIr::Map { key, value } => {
            let Ty::Scalar(k) = resolve_ty(decls, scope, key)? else {
                return Err(BackendError::Unsupported("non-scalar map key".into()));
            };
            (resolve_ty(decls, scope, value)?, Card::Map(k))
        }
    };
    Ok(Field {
        id: f.id,
        name: f.name.clone(),
        ty,
        card,
    })
}

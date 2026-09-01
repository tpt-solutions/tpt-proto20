//! Lowers the `.proto` AST into `tpt20_ir::PackageIr` (spec §10.1).
//!
//! This module implements the mapping between protobuf constructs and the
//! neutral tpt20 IR so that imported `.proto` schemas can participate in the
//! full compiler pipeline.

use std::collections::HashMap;

use tpt20_ir as ir;

use crate::proto_ast::*;
use crate::ProtoError;

/// Lowers a parsed `.proto` file into a `PackageIr`.
pub fn lower(proto: ProtoFile) -> Result<ir::PackageIr, ProtoError> {
    let mut ctx = LowerCtx::new(proto);
    let mut pkg = ir::PackageIr::default();

    pkg.name = ctx.proto.package.clone();
    pkg.imports = ctx.proto.imports.iter().map(|i| i.path.clone()).collect();

    for msg in &ctx.proto.messages {
        pkg.messages.push(lower_message(msg.clone(), &mut ctx, "")?);
    }
    for en in &ctx.proto.enums {
        pkg.enums.push(lower_enum(en.clone(), &mut ctx, "")?);
    }
    for svc in &ctx.proto.services {
        pkg.services.push(lower_service(svc.clone(), &mut ctx)?);
    }
    pkg.reserved = ctx.proto.reserved.iter().map(lower_reserved).collect();

    Ok(pkg)
}

struct LowerCtx {
    proto: ProtoFile,
    type_map: HashMap<String, ir::TypeRefIr>,
}

impl LowerCtx {
    fn new(proto: ProtoFile) -> Self {
        LowerCtx {
            proto,
            type_map: HashMap::new(),
        }
    }
}

fn lower_message(
    msg: crate::proto_ast::Message,
    ctx: &mut LowerCtx,
    parent_prefix: &str,
) -> Result<ir::MessageIr, ProtoError> {
    let full_name = if parent_prefix.is_empty() {
        msg.name.clone()
    } else {
        format!("{}.{}", parent_prefix, msg.name)
    };
    let msg_name = msg.name.clone();

    let mut ir_msg = ir::MessageIr {
        name: msg.name,
        fields: Vec::new(),
        oneofs: Vec::new(),
        messages: Vec::new(),
        enums: Vec::new(),
        reserved: msg.reserved.into_iter().map(lower_reserved).collect(),
        annotations: Vec::new(),
        span: Default::default(),
    };

    for field in msg.fields {
        let label = match field.label {
            FieldLabel::Singular => ir::FieldLabelIr::Singular(lower_type(field.field_type, ctx, &full_name)?),
            FieldLabel::Repeated => ir::FieldLabelIr::Repeated(lower_type(field.field_type, ctx, &full_name)?),
            FieldLabel::Optional => ir::FieldLabelIr::Singular(lower_type(field.field_type, ctx, &full_name)?),
            FieldLabel::Required => ir::FieldLabelIr::Singular(lower_type(field.field_type, ctx, &full_name)?),
        };
        let presence = match field.label {
            FieldLabel::Required | FieldLabel::Optional => ir::Presence::Explicit,
            _ => ir::Presence::Implicit,
        };
        ir_msg.fields.push(ir::FieldIr {
            id: field.number,
            name: field.name,
            label,
            presence,
            annotations: Vec::new(),
            span: Default::default(),
        });
    }

    for oneof in msg.oneofs {
        let mut ir_oneof = ir::OneofIr {
            name: oneof.name,
            fields: Vec::new(),
            annotations: Vec::new(),
            span: Default::default(),
        };
        for field in oneof.fields {
            let label = match field.label {
                FieldLabel::Singular => ir::FieldLabelIr::Singular(lower_type(field.field_type, ctx, &full_name)?),
                FieldLabel::Repeated => ir::FieldLabelIr::Repeated(lower_type(field.field_type, ctx, &full_name)?),
                FieldLabel::Optional => ir::FieldLabelIr::Singular(lower_type(field.field_type, ctx, &full_name)?),
                FieldLabel::Required => ir::FieldLabelIr::Singular(lower_type(field.field_type, ctx, &full_name)?),
            };
            let presence = match field.label {
                FieldLabel::Required | FieldLabel::Optional => ir::Presence::Explicit,
                _ => ir::Presence::Implicit,
            };
            ir_oneof.fields.push(ir::FieldIr {
                id: field.number,
                name: field.name,
                label,
                presence,
                annotations: Vec::new(),
                span: Default::default(),
            });
        }
        ir_msg.oneofs.push(ir_oneof);
    }

    let child_prefix = format!("{}.{}", parent_prefix, msg_name);
    for child_msg in msg.messages {
        ir_msg.messages.push(lower_message(child_msg, ctx, &child_prefix)?);
    }
    for en in msg.enums {
        ir_msg.enums.push(lower_enum(en, ctx, &child_prefix)?);
    }

    Ok(ir_msg)
}

fn lower_type(
    t: ProtoType,
    ctx: &mut LowerCtx,
    current_scope: &str,
) -> Result<ir::TypeRefIr, ProtoError> {
    match t {
        ProtoType::Double => Ok(ir::TypeRefIr { path: vec!["double".into()] }),
        ProtoType::Float => Ok(ir::TypeRefIr { path: vec!["float".into()] }),
        ProtoType::Int32 => Ok(ir::TypeRefIr { path: vec!["int32".into()] }),
        ProtoType::Int64 => Ok(ir::TypeRefIr { path: vec!["int64".into()] }),
        ProtoType::UInt32 => Ok(ir::TypeRefIr { path: vec!["uint32".into()] }),
        ProtoType::UInt64 => Ok(ir::TypeRefIr { path: vec!["uint64".into()] }),
        ProtoType::SInt32 => Ok(ir::TypeRefIr { path: vec!["sint32".into()] }),
        ProtoType::SInt64 => Ok(ir::TypeRefIr { path: vec!["sint64".into()] }),
        ProtoType::Fixed32 => Ok(ir::TypeRefIr { path: vec!["fixed32".into()] }),
        ProtoType::Fixed64 => Ok(ir::TypeRefIr { path: vec!["fixed64".into()] }),
        ProtoType::SFixed32 => Ok(ir::TypeRefIr { path: vec!["sfixed32".into()] }),
        ProtoType::SFixed64 => Ok(ir::TypeRefIr { path: vec!["sfixed64".into()] }),
        ProtoType::Bool => Ok(ir::TypeRefIr { path: vec!["bool".into()] }),
        ProtoType::String => Ok(ir::TypeRefIr { path: vec!["string".into()] }),
        ProtoType::Bytes => Ok(ir::TypeRefIr { path: vec!["bytes".into()] }),
        ProtoType::Message { name } => {
            let full = if name.len() == 1 && !name[0].contains('.') {
                if !current_scope.is_empty() {
                    format!("{}.{}", current_scope, name[0])
                } else {
                    name[0].clone()
                }
            } else {
                name.join(".")
            };
            Ok(ir::TypeRefIr { path: full.split('.').map(|s| s.to_string()).collect() })
        }
        ProtoType::Enum { name } => {
            let full = if name.len() == 1 && !name[0].contains('.') {
                if !current_scope.is_empty() {
                    format!("{}.{}", current_scope, name[0])
                } else {
                    name[0].clone()
                }
            } else {
                name.join(".")
            };
            Ok(ir::TypeRefIr { path: full.split('.').map(|s| s.to_string()).collect() })
        }
        ProtoType::Map { key, value } => {
            let key_path = lower_type(*key, ctx, current_scope)?.path;
            let val_path = lower_type(*value, ctx, current_scope)?.path;
            Ok(ir::TypeRefIr {
                path: vec![format!("map<{},{}>", key_path.join("."), val_path.join("."))],
            })
        }
    }
}

fn lower_enum(
    en: crate::proto_ast::Enum,
    _ctx: &mut LowerCtx,
    _parent_prefix: &str,
) -> Result<ir::EnumIr, ProtoError> {
    let mut ir_en = ir::EnumIr {
        name: en.name,
        values: Vec::new(),
        open: false,
        annotations: Vec::new(),
        span: Default::default(),
    };

    for v in en.values {
        ir_en.values.push(ir::EnumValueIr {
            name: v.name,
            number: v.number,
            alias: false,
        });
    }

    Ok(ir_en)
}

fn lower_service(
    svc: crate::proto_ast::Service,
    _ctx: &mut LowerCtx,
) -> Result<ir::ServiceIr, ProtoError> {
    let mut ir_svc = ir::ServiceIr {
        name: svc.name,
        methods: Vec::new(),
        annotations: Vec::new(),
        span: Default::default(),
    };

    for m in svc.methods {
        ir_svc.methods.push(ir::MethodIr {
            name: m.name,
            request: ir::TypeRefIr { path: m.request_type },
            request_streaming: m.request_streaming,
            response: ir::TypeRefIr { path: m.response_type },
            response_streaming: m.response_streaming,
            annotations: Vec::new(),
        });
    }

    Ok(ir_svc)
}

fn lower_reserved(r: crate::proto_ast::Reserved) -> ir::ReservedIr {
    let mut ids = Vec::new();
    for id in r.ids {
        match id {
            ReservedId::Single(n) => ids.push(ir::ReservedIdIr::Single(n)),
            ReservedId::Range(a, b) => ids.push(ir::ReservedIdIr::Range(a, b)),
        }
    }
    ir::ReservedIr {
        ids,
        names: r.names,
    }
}

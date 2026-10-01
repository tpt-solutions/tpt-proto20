//! Schema-aware conversion between protobuf bytes and tpt20 native bytes.
//!
//! The plain [`wire`](crate::wire) adapter only translates the tags of one
//! message level. Nested messages and map entries travel inside
//! length-delimited payloads that carry their *own* tags, so a correct
//! conversion has to know, from the schema, which payloads are messages.
//! These functions walk the schema and convert every level.
//!
//! Scalar encodings already agree between the formats (varints, zigzag,
//! fixed-width little-endian, packed repeated), so only tags change.

use crate::error::WireError;
use crate::wire::{decode_protobuf_with, encode_protobuf};
use std::collections::HashMap;
use tpt20_core::limits::{DecoderLimits, UnknownFieldPolicy};
use tpt20_core::message::{Field, RawMessage, Value};
use tpt20_core::wire::WireClass;
use tpt20_ir as ir;

#[derive(Clone, Copy)]
enum Dir {
    ProtoToNative,
    NativeToProto,
}

/// What a length-delimited payload of a field holds.
#[derive(Clone)]
enum Payload<'a> {
    Message(&'a ir::MessageIr, String),
    /// Map entry: value is a message (`Some`) or a scalar/enum (`None`).
    MapEntry(Option<(&'a ir::MessageIr, String)>),
}

struct Schema<'a> {
    messages: HashMap<String, &'a ir::MessageIr>,
    package: String,
}

impl<'a> Schema<'a> {
    fn new(pkg: &'a ir::PackageIr) -> Self {
        fn walk<'a>(
            m: &'a ir::MessageIr,
            prefix: &str,
            out: &mut HashMap<String, &'a ir::MessageIr>,
        ) {
            let name = format!("{prefix}.{}", m.name);
            out.insert(name.clone(), m);
            for n in &m.messages {
                walk(n, &name, out);
            }
        }
        let package = pkg.name.clone().unwrap_or_default();
        let mut messages = HashMap::new();
        for m in &pkg.messages {
            walk(m, &package, &mut messages);
        }
        Schema { messages, package }
    }

    /// Resolves a type path used inside `scope` (a fully qualified message
    /// name) to a message, if it names one.
    fn resolve(&self, scope: &str, path: &[String]) -> Option<(&'a ir::MessageIr, String)> {
        let rel = path.join(".");
        let mut scope = scope.to_string();
        loop {
            let cand = format!("{scope}.{rel}");
            if let Some(m) = self.messages.get(&cand) {
                return Some((m, cand));
            }
            let i = scope.rfind('.')?;
            scope.truncate(i);
        }
    }

    fn lookup(&self, name: &str) -> Result<(&'a ir::MessageIr, String), WireError> {
        let full = if self.package.is_empty() {
            format!(".{name}")
        } else if name.starts_with(&format!("{}.", self.package)) {
            name.to_string()
        } else {
            format!("{}.{name}", self.package)
        };
        let full = full.trim_start_matches('.').to_string();
        self.messages
            .get(&full)
            .map(|m| (*m, full.clone()))
            .ok_or_else(|| WireError::Schema(format!("unknown message `{name}`")))
    }

    /// Classifies the length-delimited payload of field `id` in `msg`.
    fn payload(&self, msg: &ir::MessageIr, scope: &str, id: u32) -> Option<Payload<'a>> {
        let field = msg
            .fields
            .iter()
            .chain(msg.oneofs.iter().flat_map(|o| o.fields.iter()))
            .find(|f| f.id == id)?;
        match &field.label {
            ir::FieldLabelIr::Singular(t) | ir::FieldLabelIr::Repeated(t) => self
                .resolve(scope, &t.path)
                .map(|(m, n)| Payload::Message(m, n)),
            ir::FieldLabelIr::Map { value, .. } => {
                Some(Payload::MapEntry(self.resolve(scope, &value.path)))
            }
        }
    }
}

fn native_raw(bytes: &[u8], limits: &DecoderLimits) -> Result<RawMessage, WireError> {
    RawMessage::decode(bytes, limits, UnknownFieldPolicy::Preserve)
        .map_err(|e| WireError::Native(e.to_string()))
}

fn convert(
    schema: &Schema<'_>,
    msg: &ir::MessageIr,
    scope: &str,
    bytes: &[u8],
    dir: Dir,
    limits: &DecoderLimits,
    depth: usize,
) -> Result<Vec<u8>, WireError> {
    if depth > limits.max_depth {
        return Err(WireError::TooDeep);
    }
    let raw = match dir {
        Dir::ProtoToNative => decode_protobuf_with(bytes, limits)?,
        Dir::NativeToProto => native_raw(bytes, limits)?,
    };
    let mut out = RawMessage::new();
    for field in raw.fields {
        let value = match (&field.value, field.wire_class) {
            (Value::Len(payload), WireClass::Len) => {
                match schema.payload(msg, scope, field.field_id) {
                    Some(Payload::Message(inner, name)) => Value::Len(convert(
                        schema,
                        inner,
                        &name,
                        payload,
                        dir,
                        limits,
                        depth + 1,
                    )?),
                    Some(Payload::MapEntry(value_msg)) => Value::Len(convert_entry(
                        schema,
                        value_msg,
                        payload,
                        dir,
                        limits,
                        depth + 1,
                    )?),
                    None => field.value.clone(),
                }
            }
            _ => field.value.clone(),
        };
        out.push(Field::new(field.field_id, field.wire_class, value));
    }
    match dir {
        Dir::ProtoToNative => out.encode().map_err(|e| WireError::Native(e.to_string())),
        Dir::NativeToProto => encode_protobuf(&out),
    }
}

/// Converts one map entry (`1: key`, `2: value`).
fn convert_entry(
    schema: &Schema<'_>,
    value_msg: Option<(&ir::MessageIr, String)>,
    bytes: &[u8],
    dir: Dir,
    limits: &DecoderLimits,
    depth: usize,
) -> Result<Vec<u8>, WireError> {
    if depth > limits.max_depth {
        return Err(WireError::TooDeep);
    }
    let raw = match dir {
        Dir::ProtoToNative => decode_protobuf_with(bytes, limits)?,
        Dir::NativeToProto => native_raw(bytes, limits)?,
    };
    let mut out = RawMessage::new();
    for field in raw.fields {
        let value = match (&field.value, field.field_id, &value_msg) {
            (Value::Len(payload), 2, Some((inner, name))) => Value::Len(convert(
                schema,
                inner,
                name,
                payload,
                dir,
                limits,
                depth + 1,
            )?),
            _ => field.value.clone(),
        };
        out.push(Field::new(field.field_id, field.wire_class, value));
    }
    match dir {
        Dir::ProtoToNative => out.encode().map_err(|e| WireError::Native(e.to_string())),
        Dir::NativeToProto => encode_protobuf(&out),
    }
}

/// Converts protobuf bytes of message `message` (as declared in `pkg`) to the
/// native tpt20 encoding, recursing through nested messages and map entries.
pub fn protobuf_to_native(
    bytes: &[u8],
    pkg: &ir::PackageIr,
    message: &str,
    limits: &DecoderLimits,
) -> Result<Vec<u8>, WireError> {
    let schema = Schema::new(pkg);
    let (msg, name) = schema.lookup(message)?;
    convert(&schema, msg, &name, bytes, Dir::ProtoToNative, limits, 1)
}

/// Converts native tpt20 bytes of message `message` to protobuf bytes,
/// recursing through nested messages and map entries.
pub fn native_to_protobuf(
    bytes: &[u8],
    pkg: &ir::PackageIr,
    message: &str,
    limits: &DecoderLimits,
) -> Result<Vec<u8>, WireError> {
    let schema = Schema::new(pkg);
    let (msg, name) = schema.lookup(message)?;
    convert(&schema, msg, &name, bytes, Dir::NativeToProto, limits, 1)
}

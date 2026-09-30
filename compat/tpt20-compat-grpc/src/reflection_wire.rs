//! `grpc.reflection.v1alpha` / `grpc.reflection.v1` wire service.
//!
//! Serves real `google.protobuf.FileDescriptorProto` bytes built from tpt20
//! IR, so stock tooling (`grpcurl -plaintext host:port list`, `describe`)
//! can discover services. The descriptors describe the **protobuf-compatible
//! shape** of each message (proto3 syntax, `map<>` as entry messages, explicit
//! presence as `proto3_optional`): use them with handlers that speak protobuf
//! bytes (see `tpt20-compat-protobuf`). tpt20's native wire format is a
//! different encoding.
//!
//! Limitations: one synthetic file per registered package, no imports, no
//! extensions (`all_extension_numbers_of_type` answers with an empty list).

use crate::server::GrpcCall;
use crate::GrpcError;
use std::collections::HashMap;
use tpt20_ir as ir;

/// Method paths served (same wire shape under both service names).
pub const METHODS: [&str; 2] = [
    "grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo",
    "grpc.reflection.v1.ServerReflection/ServerReflectionInfo",
];

// ---- minimal protobuf writer/reader --------------------------------------

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_tag(out: &mut Vec<u8>, field: u32, wire: u8) {
    put_varint(out, ((field as u64) << 3) | wire as u64);
}

fn put_bytes(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    put_tag(out, field, 2);
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn put_str(out: &mut Vec<u8>, field: u32, s: &str) {
    put_bytes(out, field, s.as_bytes());
}

fn put_int(out: &mut Vec<u8>, field: u32, v: i64) {
    put_tag(out, field, 0);
    put_varint(out, v as u64);
}

fn get_varint(buf: &mut &[u8]) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..70).step_by(7) {
        let (&b, rest) = buf.split_first()?;
        *buf = rest;
        v |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Returns `(field, length-delimited payload)` pairs; other wire types are
/// skipped. `None` on malformed input.
fn parse_len_fields(mut buf: &[u8]) -> Option<Vec<(u32, Vec<u8>)>> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        let tag = get_varint(&mut buf)?;
        let (field, wire) = ((tag >> 3) as u32, (tag & 7) as u8);
        match wire {
            0 => {
                get_varint(&mut buf)?;
            }
            1 | 5 => {
                let n = if wire == 1 { 8 } else { 4 };
                buf = buf.get(n..)?;
            }
            2 => {
                let len = usize::try_from(get_varint(&mut buf)?).ok()?;
                let payload = buf.get(..len)?;
                buf = &buf[len..];
                out.push((field, payload.to_vec()));
            }
            _ => return None,
        }
    }
    Some(out)
}

// ---- descriptor building -------------------------------------------------

fn scalar_type(name: &str) -> Option<i64> {
    Some(match name {
        "float64" => 1,
        "float32" => 2,
        "int64" => 3,
        "uint64" => 4,
        "int32" => 5,
        "fixed64" => 6,
        "fixed32" => 7,
        "bool" => 8,
        "string" => 9,
        "bytes" => 12,
        "uint32" => 13,
        "sfixed32" => 15,
        "sfixed64" => 16,
        "sint32" => 17,
        "sint64" => 18,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Message,
    Enum,
}

/// Fully-qualified (dotless-prefix) names of all declared types.
fn declared(pkg: &ir::PackageIr, prefix: &str) -> HashMap<String, Kind> {
    fn walk(m: &ir::MessageIr, prefix: &str, out: &mut HashMap<String, Kind>) {
        let name = format!("{prefix}.{}", m.name);
        out.insert(name.clone(), Kind::Message);
        for e in &m.enums {
            out.insert(format!("{name}.{}", e.name), Kind::Enum);
        }
        for n in &m.messages {
            walk(n, &name, out);
        }
    }
    let mut out = HashMap::new();
    for m in &pkg.messages {
        walk(m, prefix, &mut out);
    }
    for e in &pkg.enums {
        out.insert(format!("{prefix}.{}", e.name), Kind::Enum);
    }
    out
}

struct Builder<'a> {
    types: &'a HashMap<String, Kind>,
    prefix: String,
}

impl Builder<'_> {
    /// Resolves `path` from `scope` outwards; returns (".qualified", kind).
    fn resolve(&self, scope: &str, path: &[String]) -> Option<(String, Kind)> {
        let rel = path.join(".");
        let mut scope = scope.to_string();
        loop {
            let cand = format!("{scope}.{rel}");
            if let Some(k) = self.types.get(&cand) {
                return Some((format!(".{cand}"), *k));
            }
            match scope.rfind('.') {
                Some(i) if scope.len() > i => scope.truncate(i),
                _ => break,
            }
            if scope.is_empty() {
                break;
            }
        }
        None
    }

    fn field(
        &self,
        scope: &str,
        f: &ir::FieldIr,
        t: &ir::TypeRefIr,
        repeated: bool,
        oneof: Option<u32>,
        type_name_override: Option<&str>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        put_str(&mut out, 1, &f.name);
        put_int(&mut out, 3, f.id as i64);
        put_int(&mut out, 4, if repeated { 3 } else { 1 });
        if let Some(name) = type_name_override {
            put_int(&mut out, 5, 11);
            put_str(&mut out, 6, name);
        } else {
            self.put_type(&mut out, scope, t);
        }
        if let Some(i) = oneof {
            put_int(&mut out, 9, i as i64);
        }
        out
    }

    fn put_type(&self, out: &mut Vec<u8>, scope: &str, t: &ir::TypeRefIr) {
        if t.path.len() == 1 {
            if let Some(code) = scalar_type(&t.path[0]) {
                put_int(out, 5, code);
                return;
            }
        }
        match self.resolve(scope, &t.path) {
            Some((name, Kind::Enum)) => {
                put_int(out, 5, 14);
                put_str(out, 6, &name);
            }
            Some((name, Kind::Message)) => {
                put_int(out, 5, 11);
                put_str(out, 6, &name);
            }
            None => {
                put_int(out, 5, 11);
                put_str(out, 6, &format!(".{}", t.path.join(".")));
            }
        }
    }

    fn message(&self, prefix: &str, m: &ir::MessageIr) -> Vec<u8> {
        let scope = format!("{prefix}.{}", m.name);
        let mut out = Vec::new();
        put_str(&mut out, 1, &m.name);
        let mut synthetic: Vec<String> = Vec::new();
        let mut map_entries: Vec<Vec<u8>> = Vec::new();
        let mut fields: Vec<Vec<u8>> = Vec::new();
        let n_real_oneofs = m.oneofs.len() as u32;
        for f in &m.fields {
            match &f.label {
                ir::FieldLabelIr::Singular(t) => {
                    let explicit = f.presence == ir::Presence::Explicit;
                    let idx = explicit.then(|| n_real_oneofs + synthetic.len() as u32);
                    let mut fb = self.field(&scope, f, t, false, idx, None);
                    if explicit {
                        synthetic.push(format!("_{}", f.name));
                        put_int(&mut fb, 17, 1);
                    }
                    fields.push(fb);
                }
                ir::FieldLabelIr::Repeated(t) => {
                    fields.push(self.field(&scope, f, t, true, None, None));
                }
                ir::FieldLabelIr::Map { key, value } => {
                    let entry = format!("{}Entry", camel(&f.name));
                    let mut e = Vec::new();
                    put_str(&mut e, 1, &entry);
                    let kf = ir::FieldIr {
                        id: 1,
                        name: "key".into(),
                        label: ir::FieldLabelIr::Singular(key.clone()),
                        presence: ir::Presence::Implicit,
                        annotations: vec![],
                        span: f.span,
                    };
                    let vf = ir::FieldIr {
                        id: 2,
                        name: "value".into(),
                        ..kf.clone()
                    };
                    put_bytes(&mut e, 2, &self.field(&scope, &kf, key, false, None, None));
                    put_bytes(
                        &mut e,
                        2,
                        &self.field(&scope, &vf, value, false, None, None),
                    );
                    let mut opts = Vec::new();
                    put_int(&mut opts, 7, 1);
                    put_bytes(&mut e, 7, &opts);
                    map_entries.push(e);
                    let tn = format!(".{scope}.{entry}");
                    fields.push(self.field(&scope, f, key, true, None, Some(&tn)));
                }
            }
        }
        for (i, o) in m.oneofs.iter().enumerate() {
            for f in &o.fields {
                let t = f.label.unwrap_type();
                fields.push(self.field(&scope, f, t, false, Some(i as u32), None));
            }
        }
        for f in &fields {
            put_bytes(&mut out, 2, f);
        }
        for n in &m.messages {
            put_bytes(&mut out, 3, &self.message(&scope, n));
        }
        for e in map_entries {
            put_bytes(&mut out, 3, &e);
        }
        for e in &m.enums {
            put_bytes(&mut out, 4, &enum_descriptor(e));
        }
        for o in &m.oneofs {
            let mut d = Vec::new();
            put_str(&mut d, 1, &o.name);
            put_bytes(&mut out, 8, &d);
        }
        for name in synthetic {
            let mut d = Vec::new();
            put_str(&mut d, 1, &name);
            put_bytes(&mut out, 8, &d);
        }
        out
    }

    fn service(&self, s: &ir::ServiceIr) -> Vec<u8> {
        let mut out = Vec::new();
        put_str(&mut out, 1, &s.name);
        for m in &s.methods {
            let q = |t: &ir::TypeRefIr| {
                self.resolve(&self.prefix, &t.path)
                    .map(|(n, _)| n)
                    .unwrap_or_else(|| format!(".{}", t.path.join(".")))
            };
            let mut d = Vec::new();
            put_str(&mut d, 1, &m.name);
            put_str(&mut d, 2, &q(&m.request));
            put_str(&mut d, 3, &q(&m.response));
            if m.request_streaming {
                put_int(&mut d, 5, 1);
            }
            if m.response_streaming {
                put_int(&mut d, 6, 1);
            }
            put_bytes(&mut out, 2, &d);
        }
        out
    }
}

fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in name.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn enum_descriptor(e: &ir::EnumIr) -> Vec<u8> {
    let mut out = Vec::new();
    put_str(&mut out, 1, &e.name);
    for v in &e.values {
        let mut d = Vec::new();
        put_str(&mut d, 1, &v.name);
        put_int(&mut d, 2, v.number as i64);
        put_bytes(&mut out, 2, &d);
    }
    out
}

/// Builds serialized `FileDescriptorProto` bytes for `pkg`, named `filename`.
pub fn file_descriptor_bytes(filename: &str, pkg: &ir::PackageIr) -> Vec<u8> {
    let package = pkg.name.clone().unwrap_or_default();
    let types = declared(pkg, &package);
    let b = Builder {
        types: &types,
        prefix: package.clone(),
    };
    let mut out = Vec::new();
    put_str(&mut out, 1, filename);
    if !package.is_empty() {
        put_str(&mut out, 2, &package);
    }
    for m in &pkg.messages {
        put_bytes(&mut out, 4, &b.message(&package, m));
    }
    for e in &pkg.enums {
        put_bytes(&mut out, 5, &enum_descriptor(e));
    }
    for s in &pkg.services {
        put_bytes(&mut out, 6, &b.service(s));
    }
    put_str(&mut out, 12, "proto3");
    out
}

// ---- the service ---------------------------------------------------------

#[derive(Debug, Clone)]
struct FileEntry {
    name: String,
    bytes: Vec<u8>,
}

/// Serves reflection requests for the registered packages.
#[derive(Debug, Clone, Default)]
pub struct ReflectionServer {
    files: Vec<FileEntry>,
    symbols: HashMap<String, usize>,
    services: Vec<String>,
}

impl ReflectionServer {
    /// Creates an empty server.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a package under `filename` (e.g. `"demo/v1/demo.proto"`).
    pub fn with_package(mut self, filename: &str, pkg: &ir::PackageIr) -> Self {
        let idx = self.files.len();
        let package = pkg.name.clone().unwrap_or_default();
        let q = |n: &str| {
            if package.is_empty() {
                n.to_string()
            } else {
                format!("{package}.{n}")
            }
        };
        for name in declared(pkg, &package).keys() {
            self.symbols.insert(name.clone(), idx);
        }
        for s in &pkg.services {
            let full = q(&s.name);
            self.symbols.insert(full.clone(), idx);
            for m in &s.methods {
                self.symbols.insert(format!("{full}.{}", m.name), idx);
            }
            self.services.push(full);
        }
        self.files.push(FileEntry {
            name: filename.to_string(),
            bytes: file_descriptor_bytes(filename, pkg),
        });
        self
    }

    /// Fully-qualified names of the registered services (sorted), as served
    /// by `list_services`.
    pub fn service_names(&self) -> Vec<String> {
        let mut v = self.services.clone();
        v.sort();
        v
    }

    /// True if `method` is a reflection method path.
    pub fn handles(method: &str) -> bool {
        METHODS.contains(&method.trim_start_matches('/'))
    }

    /// Answers one serialized `ServerReflectionRequest`.
    pub fn respond(&self, request: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        put_str(&mut out, 1, "");
        put_bytes(&mut out, 2, request);
        let Some(fields) = parse_len_fields(request) else {
            return error_response(out, 3, "malformed reflection request");
        };
        let Some((field, arg)) = fields.into_iter().find(|(f, _)| matches!(*f, 3..=7)) else {
            return error_response(out, 12, "unsupported reflection request");
        };
        let arg = String::from_utf8_lossy(&arg).into_owned();
        match field {
            3 => match self.files.iter().find(|f| f.name == arg) {
                Some(f) => file_response(out, &[&f.bytes]),
                None => error_response(out, 5, &format!("file not found: {arg}")),
            },
            4 => match self.symbols.get(arg.trim_start_matches('.')) {
                Some(&i) => file_response(out, &[&self.files[i].bytes]),
                None => error_response(out, 5, &format!("symbol not found: {arg}")),
            },
            6 => {
                // all_extension_numbers_of_type: no extensions are known.
                let mut body = Vec::new();
                put_str(&mut body, 1, &arg);
                put_bytes(&mut out, 5, &body);
                out
            }
            7 => {
                let mut list = Vec::new();
                for s in self.service_names() {
                    let mut svc = Vec::new();
                    put_str(&mut svc, 1, &s);
                    put_bytes(&mut list, 1, &svc);
                }
                put_bytes(&mut out, 6, &list);
                out
            }
            _ => error_response(out, 12, "extension lookup is not supported"),
        }
    }

    /// Drives one `ServerReflectionInfo` stream: answers every request
    /// message until the client half-closes.
    pub async fn serve_call(&self, mut call: GrpcCall) -> Result<(), GrpcError> {
        let first = std::mem::take(&mut call.payload);
        call.send_ok(self.respond(&first)).await?;
        while let Some(req) = call.recv_message().await {
            call.send_ok(self.respond(&req)).await?;
        }
        Ok(())
    }
}

fn file_response(mut out: Vec<u8>, files: &[&Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for f in files {
        put_bytes(&mut body, 1, f);
    }
    put_bytes(&mut out, 4, &body);
    out
}

fn error_response(mut out: Vec<u8>, code: i64, message: &str) -> Vec<u8> {
    let mut body = Vec::new();
    put_int(&mut body, 1, code);
    put_str(&mut body, 2, message);
    put_bytes(&mut out, 7, &body);
    out
}

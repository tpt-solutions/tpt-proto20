//! Descriptor model and serialization for tpt20 (spec §8).
//!
//! A [`Descriptor`] wraps the neutral [`tpt20_ir::PackageIr`] and provides:
//! - JSON serialization (`to_json` / `from_json`)
//! - Binary serialization (`to_binary` / `from_binary`) with a stable,
//!   deterministic, self-describing layout
//! - Dynamic lookup by name and id (consumed by reflection, Phase 7)

use serde::{Deserialize, Serialize};
use tpt20_ir as ir;

/// A compiled, serializable schema descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    /// The neutral IR package this descriptor represents.
    pub package: ir::PackageIr,
}

impl Descriptor {
    /// Wraps a package IR as a descriptor.
    pub fn new(package: ir::PackageIr) -> Descriptor {
        Descriptor { package }
    }

    /// Serializes the descriptor to JSON.
    pub fn to_json(&self) -> Result<String, DescriptorError> {
        serde_json::to_string_pretty(&self.package).map_err(DescriptorError::Json)
    }

    /// Parses a descriptor from JSON.
    pub fn from_json(json: &str) -> Result<Descriptor, DescriptorError> {
        let package = serde_json::from_str(json).map_err(DescriptorError::Json)?;
        Ok(Descriptor { package })
    }

    /// Serializes the descriptor to the native binary interchange format.
    pub fn to_binary(&self) -> Result<Vec<u8>, DescriptorError> {
        let mut w = BinWriter::new();
        w.package(&self.package);
        Ok(w.into_bytes())
    }

    /// Parses a descriptor from the native binary interchange format.
    pub fn from_binary(bytes: &[u8]) -> Result<Descriptor, DescriptorError> {
        let mut r = BinReader::new(bytes)?;
        let package = r.package()?;
        Ok(Descriptor { package })
    }

    /// Computes and records the stable fingerprint for this descriptor's
    /// package, returning the fingerprint string.
    pub fn compute_fingerprint(&mut self) -> String {
        let fp = ir::fingerprint(&self.package);
        self.package.fingerprint = Some(fp.clone());
        fp
    }

    /// Looks up a top-level message by name.
    pub fn find_message(&self, name: &str) -> Option<&ir::MessageIr> {
        self.package.messages.iter().find(|m| m.name == name)
    }

    /// Looks up a top-level enum by name.
    pub fn find_enum(&self, name: &str) -> Option<&ir::EnumIr> {
        self.package.enums.iter().find(|e| e.name == name)
    }

    /// Looks up a top-level service by name.
    pub fn find_service(&self, name: &str) -> Option<&ir::ServiceIr> {
        self.package.services.iter().find(|s| s.name == name)
    }
}

/// Descriptor (de)serialization errors.
#[derive(Debug)]
pub enum DescriptorError {
    /// JSON (de)serialization failed.
    Json(serde_json::Error),
    /// Binary decoding failed (truncated or malformed).
    Binary(&'static str),
}

impl std::fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DescriptorError::Json(e) => write!(f, "descriptor json error: {e}"),
            DescriptorError::Binary(e) => write!(f, "descriptor binary error: {e}"),
        }
    }
}

impl std::error::Error for DescriptorError {}

// ----------------------------------------------------------------------------
// Compact, deterministic binary codec.
//
// Layout: 4-byte magic "TPD1", then fields encoded in a fixed, documented order
// so the output is reproducible (suitable for hashing, signing, and cross-language
// interchange). Repeats are length-prefixed. Strings are UTF-8 length-prefixed.
// ----------------------------------------------------------------------------

const MAGIC: &[u8; 4] = b"TPD1";

struct BinWriter {
    buf: Vec<u8>,
}

impl BinWriter {
    fn new() -> BinWriter {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        BinWriter { buf }
    }

    fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    fn varint(&mut self, v: u64) {
        let mut v = v;
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                self.buf.push(byte | 0x80);
            } else {
                self.buf.push(byte);
                break;
            }
        }
    }

    fn str(&mut self, s: &str) {
        self.varint(s.len() as u64);
        self.buf.extend_from_slice(s.as_bytes());
    }

    fn bool(&mut self, b: bool) {
        self.buf.push(if b { 1 } else { 0 });
    }

    fn opt_str(&mut self, s: &Option<String>) {
        match s {
            Some(s) => {
                self.buf.push(1);
                self.str(s);
            }
            None => self.buf.push(0),
        }
    }

    fn strings(&mut self, v: &[String]) {
        self.varint(v.len() as u64);
        for s in v {
            self.str(s);
        }
    }

    fn path(&mut self, p: &[String]) {
        self.strings(p);
    }

    fn type_ref(&mut self, t: &ir::TypeRefIr) {
        self.path(&t.path);
    }

    fn annotation_arg(&mut self, a: &ir::AnnotationArgIr) {
        match a {
            ir::AnnotationArgIr::Ident(s) => {
                self.buf.push(0);
                self.str(s);
            }
            ir::AnnotationArgIr::String(s) => {
                self.buf.push(1);
                self.str(s);
            }
            ir::AnnotationArgIr::Int(n) => {
                self.buf.push(2);
                self.varint(*n as u64);
            }
            ir::AnnotationArgIr::Bool(b) => {
                self.buf.push(3);
                self.bool(*b);
            }
        }
    }

    fn annotation(&mut self, a: &ir::AnnotationIr) {
        self.str(&a.name);
        self.varint(a.args.len() as u64);
        for arg in &a.args {
            self.annotation_arg(arg);
        }
    }

    fn annotations(&mut self, v: &[ir::AnnotationIr]) {
        self.varint(v.len() as u64);
        for a in v {
            self.annotation(a);
        }
    }

    fn span(&mut self, s: &ir::SourceSpan) {
        self.varint(s.line as u64);
        self.varint(s.column as u64);
    }

    fn field_label(&mut self, l: &ir::FieldLabelIr) {
        match l {
            ir::FieldLabelIr::Singular(t) => {
                self.buf.push(0);
                self.type_ref(t);
            }
            ir::FieldLabelIr::Repeated(t) => {
                self.buf.push(1);
                self.type_ref(t);
            }
            ir::FieldLabelIr::Map { key, value } => {
                self.buf.push(2);
                self.type_ref(key);
                self.type_ref(value);
            }
        }
    }

    fn field(&mut self, f: &ir::FieldIr) {
        self.varint(f.id as u64);
        self.str(&f.name);
        self.field_label(&f.label);
        self.buf.push(f.presence as u8);
        self.annotations(&f.annotations);
        self.span(&f.span);
    }

    fn oneof(&mut self, o: &ir::OneofIr) {
        self.str(&o.name);
        self.varint(o.fields.len() as u64);
        for f in &o.fields {
            self.field(f);
        }
        self.annotations(&o.annotations);
        self.span(&o.span);
    }

    fn enum_value(&mut self, v: &ir::EnumValueIr) {
        self.str(&v.name);
        self.varint(v.number as u64);
        self.buf.push(if v.alias { 1 } else { 0 });
    }

    fn enumeration(&mut self, e: &ir::EnumIr) {
        self.str(&e.name);
        self.bool(e.open);
        self.annotations(&e.annotations);
        self.varint(e.values.len() as u64);
        for v in &e.values {
            self.enum_value(v);
        }
        self.span(&e.span);
    }

    fn method(&mut self, m: &ir::MethodIr) {
        self.str(&m.name);
        self.type_ref(&m.request);
        self.bool(m.request_streaming);
        self.type_ref(&m.response);
        self.bool(m.response_streaming);
        self.annotations(&m.annotations);
    }

    fn service(&mut self, s: &ir::ServiceIr) {
        self.str(&s.name);
        self.annotations(&s.annotations);
        self.span(&s.span);
        self.varint(s.methods.len() as u64);
        for m in &s.methods {
            self.method(m);
        }
    }

    fn message(&mut self, m: &ir::MessageIr) {
        self.str(&m.name);
        self.varint(m.fields.len() as u64);
        for f in &m.fields {
            self.field(f);
        }
        self.varint(m.oneofs.len() as u64);
        for o in &m.oneofs {
            self.oneof(o);
        }
        self.varint(m.messages.len() as u64);
        for n in &m.messages {
            self.message(n);
        }
        self.varint(m.enums.len() as u64);
        for e in &m.enums {
            self.enumeration(e);
        }
        self.reserved(&m.reserved);
        self.annotations(&m.annotations);
        self.span(&m.span);
    }

    fn reserved_id(&mut self, r: &ir::ReservedIdIr) {
        match r {
            ir::ReservedIdIr::Single(n) => {
                self.buf.push(0);
                self.varint(*n as u64);
            }
            ir::ReservedIdIr::Range(lo, hi) => {
                self.buf.push(1);
                self.varint(*lo as u64);
                self.varint(*hi as u64);
            }
        }
    }

    fn reserved(&mut self, v: &[ir::ReservedIr]) {
        self.varint(v.len() as u64);
        for r in v {
            self.varint(r.ids.len() as u64);
            for id in &r.ids {
                self.reserved_id(id);
            }
            self.strings(&r.names);
        }
    }

    fn compat(&mut self, c: &ir::CompatMetadata) {
        self.str(&c.policy);
        self.strings(&c.versions);
        self.strings(&c.deprecations);
    }

    fn package(&mut self, p: &ir::PackageIr) {
        self.opt_str(&p.name);
        self.strings(&p.imports);
        self.varint(p.messages.len() as u64);
        for m in &p.messages {
            self.message(m);
        }
        self.varint(p.enums.len() as u64);
        for e in &p.enums {
            self.enumeration(e);
        }
        self.varint(p.services.len() as u64);
        for s in &p.services {
            self.service(s);
        }
        self.reserved(&p.reserved);
        self.compat(&p.compat);
        self.opt_str(&p.fingerprint);
    }
}

struct BinReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> BinReader<'a> {
    fn new(bytes: &'a [u8]) -> Result<BinReader<'a>, DescriptorError> {
        if bytes.len() < 4 || &bytes[..4] != MAGIC {
            return Err(DescriptorError::Binary("bad magic"));
        }
        Ok(BinReader { bytes, pos: 4 })
    }

    fn varint(&mut self) -> Result<u64, DescriptorError> {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            if self.pos >= self.bytes.len() {
                return Err(DescriptorError::Binary("truncated varint"));
            }
            let byte = self.bytes[self.pos];
            self.pos += 1;
            result |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                return Err(DescriptorError::Binary("varint overflow"));
            }
        }
    }

    fn str(&mut self) -> Result<String, DescriptorError> {
        let len = self.varint()? as usize;
        if self.pos + len > self.bytes.len() {
            return Err(DescriptorError::Binary("truncated string"));
        }
        let s = String::from_utf8(self.bytes[self.pos..self.pos + len].to_vec())
            .map_err(|_| DescriptorError::Binary("invalid utf8"))?;
        self.pos += len;
        Ok(s)
    }

    fn bool(&mut self) -> Result<bool, DescriptorError> {
        if self.pos >= self.bytes.len() {
            return Err(DescriptorError::Binary("truncated bool"));
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Ok(b != 0)
    }

    fn opt_str(&mut self) -> Result<Option<String>, DescriptorError> {
        if self.bool()? {
            Ok(Some(self.str()?))
        } else {
            Ok(None)
        }
    }

    fn strings(&mut self) -> Result<Vec<String>, DescriptorError> {
        let n = self.varint()? as usize;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.str()?);
        }
        Ok(v)
    }

    fn path(&mut self) -> Result<Vec<String>, DescriptorError> {
        self.strings()
    }

    fn type_ref(&mut self) -> Result<ir::TypeRefIr, DescriptorError> {
        Ok(ir::TypeRefIr { path: self.path()? })
    }

    fn annotation_arg(&mut self) -> Result<ir::AnnotationArgIr, DescriptorError> {
        let tag = self
            .bytes
            .get(self.pos)
            .copied()
            .ok_or(DescriptorError::Binary("truncated"))?;
        self.pos += 1;
        match tag {
            0 => Ok(ir::AnnotationArgIr::Ident(self.str()?)),
            1 => Ok(ir::AnnotationArgIr::String(self.str()?)),
            2 => Ok(ir::AnnotationArgIr::Int(self.varint()? as i64)),
            3 => Ok(ir::AnnotationArgIr::Bool(self.bool()?)),
            _ => Err(DescriptorError::Binary("bad annotation arg tag")),
        }
    }

    fn annotation(&mut self) -> Result<ir::AnnotationIr, DescriptorError> {
        let name = self.str()?;
        let n = self.varint()? as usize;
        let mut args = Vec::with_capacity(n);
        for _ in 0..n {
            args.push(self.annotation_arg()?);
        }
        Ok(ir::AnnotationIr { name, args })
    }

    fn annotations(&mut self) -> Result<Vec<ir::AnnotationIr>, DescriptorError> {
        let n = self.varint()? as usize;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.annotation()?);
        }
        Ok(v)
    }

    fn span(&mut self) -> Result<ir::SourceSpan, DescriptorError> {
        Ok(ir::SourceSpan {
            line: self.varint()? as usize,
            column: self.varint()? as usize,
        })
    }

    fn field_label(&mut self) -> Result<ir::FieldLabelIr, DescriptorError> {
        let tag = self
            .bytes
            .get(self.pos)
            .copied()
            .ok_or(DescriptorError::Binary("truncated"))?;
        self.pos += 1;
        match tag {
            0 => Ok(ir::FieldLabelIr::Singular(self.type_ref()?)),
            1 => Ok(ir::FieldLabelIr::Repeated(self.type_ref()?)),
            2 => Ok(ir::FieldLabelIr::Map {
                key: self.type_ref()?,
                value: self.type_ref()?,
            }),
            _ => Err(DescriptorError::Binary("bad field label tag")),
        }
    }

    fn field(&mut self) -> Result<ir::FieldIr, DescriptorError> {
        Ok(ir::FieldIr {
            id: self.varint()? as u32,
            name: self.str()?,
            label: self.field_label()?,
            presence: if self.bool()? {
                ir::Presence::Explicit
            } else {
                ir::Presence::Implicit
            },
            annotations: self.annotations()?,
            span: self.span()?,
        })
    }

    fn oneof(&mut self) -> Result<ir::OneofIr, DescriptorError> {
        Ok(ir::OneofIr {
            name: self.str()?,
            fields: {
                let n = self.varint()? as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.field()?);
                }
                v
            },
            annotations: self.annotations()?,
            span: self.span()?,
        })
    }

    fn enum_value(&mut self) -> Result<ir::EnumValueIr, DescriptorError> {
        Ok(ir::EnumValueIr {
            name: self.str()?,
            number: self.varint()? as i32,
            alias: self.bool()?,
        })
    }

    fn enumeration(&mut self) -> Result<ir::EnumIr, DescriptorError> {
        let name = self.str()?;
        let open = self.bool()?;
        let annotations = self.annotations()?;
        let n = self.varint()? as usize;
        let mut values = Vec::with_capacity(n);
        for _ in 0..n {
            values.push(self.enum_value()?);
        }
        let span = self.span()?;
        Ok(ir::EnumIr {
            name,
            values,
            open,
            annotations,
            span,
        })
    }

    fn method(&mut self) -> Result<ir::MethodIr, DescriptorError> {
        Ok(ir::MethodIr {
            name: self.str()?,
            request: self.type_ref()?,
            request_streaming: self.bool()?,
            response: self.type_ref()?,
            response_streaming: self.bool()?,
            annotations: self.annotations()?,
        })
    }

    fn service(&mut self) -> Result<ir::ServiceIr, DescriptorError> {
        let name = self.str()?;
        let annotations = self.annotations()?;
        let span = self.span()?;
        let n = self.varint()? as usize;
        let mut methods = Vec::with_capacity(n);
        for _ in 0..n {
            methods.push(self.method()?);
        }
        Ok(ir::ServiceIr {
            name,
            methods,
            annotations,
            span,
        })
    }

    fn message(&mut self) -> Result<ir::MessageIr, DescriptorError> {
        let name = self.str()?;
        let n = self.varint()? as usize;
        let mut fields = Vec::with_capacity(n);
        for _ in 0..n {
            fields.push(self.field()?);
        }
        let n = self.varint()? as usize;
        let mut oneofs = Vec::with_capacity(n);
        for _ in 0..n {
            oneofs.push(self.oneof()?);
        }
        let n = self.varint()? as usize;
        let mut messages = Vec::with_capacity(n);
        for _ in 0..n {
            messages.push(self.message()?);
        }
        let n = self.varint()? as usize;
        let mut enums = Vec::with_capacity(n);
        for _ in 0..n {
            enums.push(self.enumeration()?);
        }
        let reserved = self.reserved()?;
        let annotations = self.annotations()?;
        let span = self.span()?;
        Ok(ir::MessageIr {
            name,
            fields,
            oneofs,
            messages,
            enums,
            reserved,
            annotations,
            span,
        })
    }

    fn reserved_id(&mut self) -> Result<ir::ReservedIdIr, DescriptorError> {
        let tag = self
            .bytes
            .get(self.pos)
            .copied()
            .ok_or(DescriptorError::Binary("truncated"))?;
        self.pos += 1;
        match tag {
            0 => Ok(ir::ReservedIdIr::Single(self.varint()? as u32)),
            1 => Ok(ir::ReservedIdIr::Range(
                self.varint()? as u32,
                self.varint()? as u32,
            )),
            _ => Err(DescriptorError::Binary("bad reserved id tag")),
        }
    }

    fn reserved(&mut self) -> Result<Vec<ir::ReservedIr>, DescriptorError> {
        let n = self.varint()? as usize;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let m = self.varint()? as usize;
            let mut ids = Vec::with_capacity(m);
            for _ in 0..m {
                ids.push(self.reserved_id()?);
            }
            let names = self.strings()?;
            out.push(ir::ReservedIr { ids, names });
        }
        Ok(out)
    }

    fn compat(&mut self) -> Result<ir::CompatMetadata, DescriptorError> {
        Ok(ir::CompatMetadata {
            policy: self.str()?,
            versions: self.strings()?,
            deprecations: self.strings()?,
        })
    }

    fn package(&mut self) -> Result<ir::PackageIr, DescriptorError> {
        let name = self.opt_str()?;
        let imports = self.strings()?;
        let n = self.varint()? as usize;
        let mut messages = Vec::with_capacity(n);
        for _ in 0..n {
            messages.push(self.message()?);
        }
        let n = self.varint()? as usize;
        let mut enums = Vec::with_capacity(n);
        for _ in 0..n {
            enums.push(self.enumeration()?);
        }
        let n = self.varint()? as usize;
        let mut services = Vec::with_capacity(n);
        for _ in 0..n {
            services.push(self.service()?);
        }
        let reserved = self.reserved()?;
        let compat = self.compat()?;
        let fingerprint = self.opt_str()?;
        Ok(ir::PackageIr {
            name,
            imports,
            messages,
            enums,
            services,
            reserved,
            compat,
            fingerprint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ir::PackageIr {
        ir::PackageIr {
            name: Some("user.v1".to_string()),
            imports: vec!["common.tpt".to_string()],
            messages: vec![ir::MessageIr {
                name: "User".to_string(),
                fields: vec![ir::FieldIr {
                    id: 1,
                    name: "id".to_string(),
                    label: ir::FieldLabelIr::Singular(ir::TypeRefIr {
                        path: vec!["int64".to_string()],
                    }),
                    presence: ir::Presence::Implicit,
                    annotations: vec![],
                    span: ir::SourceSpan { line: 3, column: 5 },
                }],
                oneofs: vec![],
                messages: vec![],
                enums: vec![],
                reserved: vec![],
                annotations: vec![],
                span: ir::SourceSpan { line: 2, column: 1 },
            }],
            enums: vec![],
            services: vec![],
            reserved: vec![],
            compat: ir::CompatMetadata::default(),
            fingerprint: None,
        }
    }

    #[test]
    fn json_roundtrip() {
        let d = Descriptor::new(sample());
        let json = d.to_json().unwrap();
        let back = Descriptor::from_json(&json).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn binary_roundtrip() {
        let d = Descriptor::new(sample());
        let bin = d.to_binary().unwrap();
        let back = Descriptor::from_binary(&bin).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn fingerprint_is_stable() {
        let mut a = Descriptor::new(sample());
        let mut b = Descriptor::new(sample());
        assert_eq!(a.compute_fingerprint(), b.compute_fingerprint());
    }

    #[test]
    fn binary_rejects_bad_magic() {
        assert!(Descriptor::from_binary(&[0, 1, 2, 3]).is_err());
    }
}

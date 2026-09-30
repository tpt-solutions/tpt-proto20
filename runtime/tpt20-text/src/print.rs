//! Text printer.

use crate::schema::{all_fields, Kind, Resolver, Scalar, TextError};
use crate::TextFormat;
use std::collections::BTreeMap;
use std::fmt::Write;
use tpt20_core::varint::decode_zigzag;
use tpt20_core::{scalar, Field, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_ir as ir;

pub(crate) fn print(
    fmt: &TextFormat<'_>,
    message: &str,
    raw: &RawMessage,
) -> Result<String, TextError> {
    let resolver = Resolver {
        package: &fmt.descriptor().package,
    };
    let chain = resolver.locate(message)?;
    let mut out = String::new();
    Printer { fmt, resolver }.message(&mut out, &chain, raw, 0, 1)?;
    Ok(out)
}

struct Printer<'f, 'a> {
    fmt: &'f TextFormat<'a>,
    resolver: Resolver<'a>,
}

/// Sort key making map output deterministic: numeric keys numerically,
/// strings/bytes bytewise.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum MapKey {
    Int(i128),
    Bytes(Vec<u8>),
}

impl<'f, 'a> Printer<'f, 'a> {
    fn message(
        &self,
        out: &mut String,
        chain: &[&'a ir::MessageIr],
        raw: &RawMessage,
        indent: usize,
        depth: usize,
    ) -> Result<(), TextError> {
        if depth > self.fmt.max_depth {
            return Err(TextError::DepthExceeded);
        }
        let msg = *chain.last().expect("non-empty chain");

        // For each oneof, the member whose last occurrence is latest wins.
        let mut winners: BTreeMap<usize, (usize, u32)> = BTreeMap::new();
        for fr in all_fields(msg) {
            if let Some(oi) = fr.oneof {
                if let Some(pos) = raw.fields.iter().rposition(|f| f.field_id == fr.field.id) {
                    let e = winners.entry(oi).or_insert((pos, fr.field.id));
                    if pos >= e.0 {
                        *e = (pos, fr.field.id);
                    }
                }
            }
        }

        for fr in all_fields(msg) {
            let field = fr.field;
            if let Some(oi) = fr.oneof {
                if winners.get(&oi).map(|w| w.1) != Some(field.id) {
                    continue;
                }
            }
            let occurrences: Vec<&Field> = raw
                .fields
                .iter()
                .filter(|f| f.field_id == field.id)
                .collect();
            if occurrences.is_empty() {
                continue;
            }
            match &field.label {
                ir::FieldLabelIr::Singular(t) => {
                    let kind = self.resolver.resolve(chain, &t.path)?;
                    let last = occurrences[occurrences.len() - 1];
                    self.value(
                        out,
                        &field.name,
                        &kind,
                        &last.value,
                        last.wire_class,
                        indent,
                        depth,
                    )?;
                }
                ir::FieldLabelIr::Repeated(t) => {
                    let kind = self.resolver.resolve(chain, &t.path)?;
                    for occ in occurrences {
                        for (v, class) in self.unpack(&field.name, &kind, occ)? {
                            self.value(out, &field.name, &kind, &v, class, indent, depth)?;
                        }
                    }
                }
                ir::FieldLabelIr::Map { key, value } => {
                    let kkind = self.resolver.resolve(chain, &key.path)?;
                    let vkind = self.resolver.resolve(chain, &value.path)?;
                    let mut entries: BTreeMap<MapKey, (Value, WireClass, Value, WireClass)> =
                        BTreeMap::new();
                    for occ in occurrences {
                        let Value::Len(bytes) = &occ.value else {
                            return Err(TextError::WireMismatch(field.name.clone()));
                        };
                        let entry = RawMessage::decode(
                            bytes,
                            fmt_limits(self.fmt),
                            UnknownFieldPolicy::Discard,
                        )
                        .map_err(|e| TextError::Decode(e.to_string()))?;
                        let k = entry.fields.iter().rev().find(|f| f.field_id == 1);
                        let v = entry.fields.iter().rev().find(|f| f.field_id == 2);
                        let (Some(k), Some(v)) = (k, v) else {
                            return Err(TextError::MalformedMapEntry(field.name.clone()));
                        };
                        let sort = map_sort_key(&field.name, &kkind, &k.value)?;
                        entries.insert(
                            sort,
                            (k.value.clone(), k.wire_class, v.value.clone(), v.wire_class),
                        );
                    }
                    for (kv, kc, vv, vc) in entries.values() {
                        pad(out, indent);
                        let _ = writeln!(out, "{} {{", field.name);
                        self.value(out, "key", &kkind, kv, *kc, indent + 2, depth)?;
                        self.value(out, "value", &vkind, vv, *vc, indent + 2, depth)?;
                        pad(out, indent);
                        out.push_str("}\n");
                    }
                }
            }
        }
        Ok(())
    }

    /// Expands packed occurrences of packable repeated scalars.
    fn unpack(
        &self,
        name: &str,
        kind: &Kind<'a>,
        occ: &Field,
    ) -> Result<Vec<(Value, WireClass)>, TextError> {
        let packable = match kind {
            Kind::Scalar(s) => s.packable().then(|| s.wire_class()),
            Kind::Enum(_) => Some(WireClass::Varint),
            Kind::Message(_) => None,
        };
        let (Some(class), WireClass::Len, Value::Len(_)) = (packable, occ.wire_class, &occ.value)
        else {
            return Ok(vec![(occ.value.clone(), occ.wire_class)]);
        };
        let limits = self.fmt.limits();
        let map_err = |e: tpt20_core::DecodeError| TextError::Decode(format!("{name}: {e}"));
        Ok(match class {
            WireClass::Varint => scalar::decode_packed_varints(&occ.value, limits)
                .map_err(map_err)?
                .into_iter()
                .map(|w| (Value::Varint(w), WireClass::Varint))
                .collect(),
            WireClass::Fixed32 => scalar::decode_packed_fixed32(&occ.value, limits)
                .map_err(map_err)?
                .into_iter()
                .map(|w| (Value::Fixed32(w), WireClass::Fixed32))
                .collect(),
            WireClass::Fixed64 => scalar::decode_packed_fixed64(&occ.value, limits)
                .map_err(map_err)?
                .into_iter()
                .map(|w| (Value::Fixed64(w), WireClass::Fixed64))
                .collect(),
            WireClass::Len => unreachable!("packable classes are fixed-width or varint"),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn value(
        &self,
        out: &mut String,
        name: &str,
        kind: &Kind<'a>,
        value: &Value,
        class: WireClass,
        indent: usize,
        depth: usize,
    ) -> Result<(), TextError> {
        match kind {
            Kind::Message(chain) => {
                let Value::Len(bytes) = value else {
                    return Err(TextError::WireMismatch(name.to_string()));
                };
                if depth + 1 > self.fmt.max_depth {
                    return Err(TextError::DepthExceeded);
                }
                let nested =
                    RawMessage::decode(bytes, fmt_limits(self.fmt), UnknownFieldPolicy::Preserve)
                        .map_err(|e| TextError::Decode(e.to_string()))?;
                pad(out, indent);
                let _ = writeln!(out, "{name} {{");
                self.message(out, chain, &nested, indent + 2, depth + 1)?;
                pad(out, indent);
                out.push_str("}\n");
            }
            Kind::Enum(e) => {
                let Value::Varint(v) = value else {
                    return Err(TextError::WireMismatch(name.to_string()));
                };
                let n = *v as i64 as i32;
                pad(out, indent);
                match e.values.iter().find(|ev| ev.number == n && !ev.alias) {
                    Some(ev) => {
                        let _ = writeln!(out, "{name}: {}", ev.name);
                    }
                    None => {
                        let _ = writeln!(out, "{name}: {n}");
                    }
                }
            }
            Kind::Scalar(s) => {
                let text = scalar_text(name, *s, value, class)?;
                pad(out, indent);
                let _ = writeln!(out, "{name}: {text}");
            }
        }
        Ok(())
    }
}

fn fmt_limits<'a>(fmt: &'a TextFormat<'_>) -> &'a tpt20_core::DecoderLimits {
    fmt.limits()
}

fn pad(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}

fn map_sort_key(name: &str, kind: &Kind<'_>, value: &Value) -> Result<MapKey, TextError> {
    let mismatch = || TextError::WireMismatch(name.to_string());
    let Kind::Scalar(s) = kind else {
        return Err(TextError::UnresolvedType(format!("map key of `{name}`")));
    };
    Ok(match (s, value) {
        (Scalar::String | Scalar::Bytes, Value::Len(b)) => MapKey::Bytes(b.clone()),
        (Scalar::Bool | Scalar::Uint32 | Scalar::Uint64, Value::Varint(v)) => {
            MapKey::Int(i128::from(*v))
        }
        (Scalar::Int32 | Scalar::Int64, Value::Varint(v)) => MapKey::Int(i128::from(*v as i64)),
        (Scalar::Sint32 | Scalar::Sint64, Value::Varint(v)) => {
            MapKey::Int(i128::from(decode_zigzag(*v)))
        }
        (Scalar::Fixed32, Value::Fixed32(v)) => MapKey::Int(i128::from(*v)),
        (Scalar::Sfixed32, Value::Fixed32(v)) => MapKey::Int(i128::from(*v as i32)),
        (Scalar::Fixed64, Value::Fixed64(v)) => MapKey::Int(i128::from(*v)),
        (Scalar::Sfixed64, Value::Fixed64(v)) => MapKey::Int(i128::from(*v as i64)),
        _ => return Err(mismatch()),
    })
}

fn scalar_text(
    name: &str,
    s: Scalar,
    value: &Value,
    class: WireClass,
) -> Result<String, TextError> {
    let mismatch = || TextError::WireMismatch(name.to_string());
    if class != s.wire_class() {
        return Err(mismatch());
    }
    Ok(match (s, value) {
        (Scalar::Bool, Value::Varint(v)) => (*v != 0).to_string(),
        (Scalar::Int32, Value::Varint(v)) => (*v as i64 as i32).to_string(),
        (Scalar::Int64, Value::Varint(v)) => (*v as i64).to_string(),
        (Scalar::Uint32, Value::Varint(v)) => (*v as u32).to_string(),
        (Scalar::Uint64, Value::Varint(v)) => v.to_string(),
        (Scalar::Sint32, Value::Varint(v)) => (decode_zigzag(*v) as i32).to_string(),
        (Scalar::Sint64, Value::Varint(v)) => decode_zigzag(*v).to_string(),
        (Scalar::Fixed32, Value::Fixed32(v)) => v.to_string(),
        (Scalar::Sfixed32, Value::Fixed32(v)) => (*v as i32).to_string(),
        (Scalar::Fixed64, Value::Fixed64(v)) => v.to_string(),
        (Scalar::Sfixed64, Value::Fixed64(v)) => (*v as i64).to_string(),
        (Scalar::Float32, Value::Fixed32(v)) => {
            let x = f32::from_bits(*v);
            float_text(f64::from(x), || x.to_string())
        }
        (Scalar::Float64, Value::Fixed64(v)) => {
            let x = f64::from_bits(*v);
            float_text(x, || x.to_string())
        }
        (Scalar::String, Value::Len(b)) => {
            let s = std::str::from_utf8(b).map_err(|_| TextError::InvalidUtf8(name.to_string()))?;
            quote(s.as_bytes(), true)
        }
        (Scalar::Bytes, Value::Len(b)) => quote(b, false),
        _ => return Err(mismatch()),
    })
}

/// Formats floats; `plain` produces the shortest round-trip form for finite
/// values (kept per-width so `0.1f32` prints `0.1`, not `0.10000000149…`).
fn float_text(f: f64, plain: impl FnOnce() -> String) -> String {
    if f.is_nan() {
        "nan".to_string()
    } else if f.is_infinite() {
        if f > 0.0 { "inf" } else { "-inf" }.to_string()
    } else {
        plain()
    }
}

/// Quotes a byte string. For `utf8` text, bytes >= 0x80 are kept raw (the
/// input is known valid UTF-8); otherwise they are `\xNN` escaped.
fn quote(bytes: &[u8], utf8: bool) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    if utf8 {
        let s = std::str::from_utf8(bytes).unwrap_or("");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                    let _ = write!(out, "\\x{:02x}", c as u32);
                }
                c => out.push(c),
            }
        }
    } else {
        for &b in bytes {
            match b {
                b'"' => out.push_str("\\\""),
                b'\\' => out.push_str("\\\\"),
                b'\n' => out.push_str("\\n"),
                b'\r' => out.push_str("\\r"),
                b'\t' => out.push_str("\\t"),
                0x20..=0x7e => out.push(b as char),
                _ => {
                    let _ = write!(out, "\\x{b:02x}");
                }
            }
        }
    }
    out.push('"');
    out
}

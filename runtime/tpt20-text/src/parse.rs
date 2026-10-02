//! Text parser.

use crate::schema::{all_fields, FieldRef, Kind, Resolver, Scalar, TextError};
use crate::TextFormat;
use std::collections::BTreeSet;
use tpt20_core::varint::encode_zigzag;
use tpt20_core::{Field, RawMessage, Value, WireClass};
use tpt20_ir as ir;

pub(crate) fn parse(
    fmt: &TextFormat<'_>,
    message: &str,
    text: &str,
) -> Result<RawMessage, TextError> {
    if text.len() > fmt.max_text_bytes {
        return Err(TextError::LimitExceeded("text size"));
    }
    let resolver = Resolver {
        package: &fmt.descriptor().package,
    };
    let chain = resolver.locate(message)?;
    let tokens = lex(text)?;
    let mut p = Parser {
        fmt,
        resolver,
        tokens,
        pos: 0,
    };
    let raw = p.message(&chain, true, 1)?;
    Ok(raw)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    /// Quoted string (raw bytes after escape processing).
    Str(Vec<u8>),
    /// Numeric literal text, sign included (`-12`, `0x1f`, `1e-3`, `-inf`).
    Num(String),
    Colon,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Sep,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    line: usize,
    col: usize,
}

fn lex(text: &str) -> Result<Vec<Token>, TextError> {
    let chars: Vec<char> = text.chars().collect();
    let (mut i, mut line, mut col) = (0usize, 1usize, 1usize);
    let mut out = Vec::new();
    let syntax = |line, col, msg: &str| TextError::Syntax {
        line,
        col,
        msg: msg.to_string(),
    };
    macro_rules! bump {
        () => {{
            if chars[i] == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            i += 1;
        }};
    }
    while i < chars.len() {
        let c = chars[i];
        let (tl, tc) = (line, col);
        match c {
            c if c.is_whitespace() => bump!(),
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    bump!();
                }
            }
            ':' => {
                bump!();
                out.push(Token {
                    tok: Tok::Colon,
                    line: tl,
                    col: tc,
                });
            }
            '{' => {
                bump!();
                out.push(Token {
                    tok: Tok::LBrace,
                    line: tl,
                    col: tc,
                });
            }
            '}' => {
                bump!();
                out.push(Token {
                    tok: Tok::RBrace,
                    line: tl,
                    col: tc,
                });
            }
            '[' => {
                bump!();
                out.push(Token {
                    tok: Tok::LBracket,
                    line: tl,
                    col: tc,
                });
            }
            ']' => {
                bump!();
                out.push(Token {
                    tok: Tok::RBracket,
                    line: tl,
                    col: tc,
                });
            }
            ',' | ';' => {
                bump!();
                out.push(Token {
                    tok: Tok::Sep,
                    line: tl,
                    col: tc,
                });
            }
            '"' | '\'' => {
                let quote = c;
                bump!();
                let mut buf: Vec<u8> = Vec::new();
                loop {
                    if i >= chars.len() {
                        return Err(syntax(tl, tc, "unterminated string"));
                    }
                    let ch = chars[i];
                    if ch == quote {
                        bump!();
                        break;
                    }
                    if ch == '\n' {
                        return Err(syntax(tl, tc, "newline in string"));
                    }
                    if ch != '\\' {
                        let mut tmp = [0u8; 4];
                        buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                        bump!();
                        continue;
                    }
                    bump!(); // backslash
                    if i >= chars.len() {
                        return Err(syntax(tl, tc, "unterminated escape"));
                    }
                    let e = chars[i];
                    bump!();
                    match e {
                        'n' => buf.push(b'\n'),
                        'r' => buf.push(b'\r'),
                        't' => buf.push(b'\t'),
                        '0' => buf.push(0),
                        '\\' => buf.push(b'\\'),
                        '"' => buf.push(b'"'),
                        '\'' => buf.push(b'\''),
                        'x' => {
                            let mut v = 0u32;
                            for _ in 0..2 {
                                let d = chars
                                    .get(i)
                                    .and_then(|c| c.to_digit(16))
                                    .ok_or_else(|| syntax(line, col, "bad \\x escape"))?;
                                v = v * 16 + d;
                                bump!();
                            }
                            buf.push(v as u8);
                        }
                        other => {
                            return Err(syntax(line, col, &format!("unknown escape `\\{other}`")))
                        }
                    }
                }
                out.push(Token {
                    tok: Tok::Str(buf),
                    line: tl,
                    col: tc,
                });
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut s = String::new();
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    s.push(chars[i]);
                    bump!();
                }
                out.push(Token {
                    tok: Tok::Ident(s),
                    line: tl,
                    col: tc,
                });
            }
            c if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' => {
                let mut s = String::new();
                s.push(c);
                bump!();
                while i < chars.len() {
                    let ch = chars[i];
                    let exp_sign = (ch == '-' || ch == '+')
                        && matches!(s.chars().last(), Some('e' | 'E'))
                        && !s.starts_with("0x")
                        && !s.starts_with("-0x");
                    if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || exp_sign {
                        s.push(ch);
                        bump!();
                    } else {
                        break;
                    }
                }
                out.push(Token {
                    tok: Tok::Num(s),
                    line: tl,
                    col: tc,
                });
            }
            other => return Err(syntax(tl, tc, &format!("unexpected character `{other}`"))),
        }
    }
    Ok(out)
}

struct Parser<'f, 'a> {
    fmt: &'f TextFormat<'a>,
    resolver: Resolver<'a>,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'f, 'a> Parser<'f, 'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn err_here(&self, msg: &str) -> TextError {
        let (line, col) = self
            .peek()
            .or_else(|| self.tokens.last())
            .map(|t| (t.line, t.col))
            .unwrap_or((1, 1));
        TextError::Syntax {
            line,
            col,
            msg: msg.to_string(),
        }
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek().map(|t| &t.tok) == Some(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn skip_seps(&mut self) {
        while self.eat(&Tok::Sep) {}
    }

    /// Parses fields until end of input (top level) or `}` (nested).
    fn message(
        &mut self,
        chain: &[&'a ir::MessageIr],
        top: bool,
        depth: usize,
    ) -> Result<RawMessage, TextError> {
        if depth > self.fmt.max_depth {
            return Err(TextError::DepthExceeded);
        }
        let msg = *chain.last().expect("non-empty chain");
        let fields = all_fields(msg);
        let mut raw = RawMessage::new();
        let mut seen: BTreeSet<u32> = BTreeSet::new();
        let mut oneofs_set: BTreeSet<usize> = BTreeSet::new();
        let max_fields = self.fmt.limits().max_field_count;

        loop {
            self.skip_seps();
            let Some(tok) = self.peek().cloned() else {
                if top {
                    break;
                }
                return Err(self.err_here("unexpected end of input, expected `}`"));
            };
            match tok.tok {
                Tok::RBrace if !top => {
                    self.pos += 1;
                    break;
                }
                Tok::Ident(name) => {
                    self.pos += 1;
                    let Some(fr) = fields.iter().find(|f| f.field.name == name) else {
                        return Err(TextError::UnknownField {
                            message: msg.name.clone(),
                            field: name,
                            line: tok.line,
                            col: tok.col,
                        });
                    };
                    self.field(chain, fr, &mut raw, &mut seen, &mut oneofs_set, depth)?;
                    if raw.fields.len() > max_fields {
                        return Err(TextError::LimitExceeded("field count"));
                    }
                }
                _ => return Err(self.err_here("expected a field name")),
            }
        }
        // Stable sort: output order is by field id, repeated elements keep
        // their text order.
        raw.fields.sort_by_key(|f| f.field_id);
        Ok(raw)
    }

    fn field(
        &mut self,
        chain: &[&'a ir::MessageIr],
        fr: &FieldRef<'a>,
        raw: &mut RawMessage,
        seen: &mut BTreeSet<u32>,
        oneofs_set: &mut BTreeSet<usize>,
        depth: usize,
    ) -> Result<(), TextError> {
        let field = fr.field;
        let msg = *chain.last().expect("non-empty chain");
        let has_colon = self.eat(&Tok::Colon);
        match &field.label {
            ir::FieldLabelIr::Singular(t) => {
                if !seen.insert(field.id) {
                    return Err(TextError::DuplicateField(field.name.clone()));
                }
                if let Some(oi) = fr.oneof {
                    if !oneofs_set.insert(oi) {
                        return Err(TextError::OneofConflict(msg.oneofs[oi].name.clone()));
                    }
                }
                let kind = self.resolver.resolve(chain, &t.path)?;
                let f = self.element(&field.name, field.id, &kind, has_colon, depth)?;
                raw.push(f);
            }
            ir::FieldLabelIr::Repeated(t) => {
                let kind = self.resolver.resolve(chain, &t.path)?;
                if self.eat(&Tok::LBracket) {
                    if !has_colon {
                        return Err(self.err_here("expected `:` before `[`"));
                    }
                    loop {
                        self.skip_seps();
                        if self.eat(&Tok::RBracket) {
                            break;
                        }
                        if self.peek().is_none() {
                            return Err(self.err_here("unterminated list"));
                        }
                        let f = self.element(&field.name, field.id, &kind, true, depth)?;
                        raw.push(f);
                        if raw.fields.len() > self.fmt.limits().max_repeated_entries {
                            return Err(TextError::LimitExceeded("repeated entries"));
                        }
                    }
                } else {
                    let f = self.element(&field.name, field.id, &kind, has_colon, depth)?;
                    raw.push(f);
                }
            }
            ir::FieldLabelIr::Map { key, value } => {
                let kkind = self.resolver.resolve(chain, &key.path)?;
                let vkind = self.resolver.resolve(chain, &value.path)?;
                let list = self.eat(&Tok::LBracket);
                loop {
                    if list {
                        self.skip_seps();
                        if self.eat(&Tok::RBracket) {
                            break;
                        }
                    }
                    let f = self.map_entry(&field.name, field.id, &kkind, &vkind, depth)?;
                    raw.push(f);
                    if !list {
                        break;
                    }
                    if self.peek().is_none() {
                        return Err(self.err_here("unterminated list"));
                    }
                }
            }
        }
        Ok(())
    }

    fn map_entry(
        &mut self,
        name: &str,
        id: u32,
        kkind: &Kind<'a>,
        vkind: &Kind<'a>,
        depth: usize,
    ) -> Result<Field, TextError> {
        if depth + 1 > self.fmt.max_depth {
            return Err(TextError::DepthExceeded);
        }
        self.eat(&Tok::Colon);
        if !self.eat(&Tok::LBrace) {
            return Err(self.err_here("expected `{` for map entry"));
        }
        let (mut key, mut value) = (None, None);
        loop {
            self.skip_seps();
            if self.eat(&Tok::RBrace) {
                break;
            }
            let Some(Token {
                tok: Tok::Ident(n), ..
            }) = self.next()
            else {
                return Err(self.err_here("expected `key` or `value`"));
            };
            let colon = self.eat(&Tok::Colon);
            match n.as_str() {
                "key" if key.is_none() => {
                    key = Some(self.element(name, 1, kkind, colon, depth + 1)?)
                }
                "value" if value.is_none() => {
                    value = Some(self.element(name, 2, vkind, colon, depth + 1)?)
                }
                _ => return Err(self.err_here("expected `key` or `value` (once each)")),
            }
        }
        let (Some(key), Some(value)) = (key, value) else {
            return Err(TextError::MalformedMapEntry(name.to_string()));
        };
        let mut entry = RawMessage::new();
        entry.push(key);
        entry.push(value);
        let bytes = entry
            .encode()
            .map_err(|e| TextError::Encode(e.to_string()))?;
        Ok(Field::new(id, WireClass::Len, Value::Len(bytes)))
    }

    /// Parses one value of `kind` into a wire field.
    fn element(
        &mut self,
        name: &str,
        id: u32,
        kind: &Kind<'a>,
        has_colon: bool,
        depth: usize,
    ) -> Result<Field, TextError> {
        match kind {
            Kind::Message(chain) => {
                if depth + 1 > self.fmt.max_depth {
                    return Err(TextError::DepthExceeded);
                }
                if !self.eat(&Tok::LBrace) {
                    return Err(TextError::TypeMismatch {
                        field: name.to_string(),
                        expected: "message `{ ... }`",
                    });
                }
                let nested = self.message(chain, false, depth + 1)?;
                let bytes = nested
                    .encode()
                    .map_err(|e| TextError::Encode(e.to_string()))?;
                Ok(Field::new(id, WireClass::Len, Value::Len(bytes)))
            }
            Kind::Enum(e) => {
                if !has_colon {
                    return Err(self.err_here("expected `:`"));
                }
                let n = match self.next().map(|t| t.tok) {
                    Some(Tok::Ident(v)) => e
                        .values
                        .iter()
                        .find(|ev| ev.name == v)
                        .map(|ev| ev.number)
                        .ok_or(TextError::InvalidEnum {
                            field: name.to_string(),
                            value: v,
                        })?,
                    Some(Tok::Num(v)) => {
                        let n = parse_int(name, &v)?;
                        let n = i32::try_from(n)
                            .map_err(|_| TextError::OutOfRange(name.to_string()))?;
                        if !e.open && !e.values.iter().any(|ev| ev.number == n) {
                            return Err(TextError::InvalidEnum {
                                field: name.to_string(),
                                value: v,
                            });
                        }
                        n
                    }
                    _ => {
                        return Err(TextError::TypeMismatch {
                            field: name.to_string(),
                            expected: "enum name or number",
                        })
                    }
                };
                Ok(Field::new(
                    id,
                    WireClass::Varint,
                    Value::Varint(i64::from(n) as u64),
                ))
            }
            Kind::Scalar(s) => {
                if !has_colon {
                    return Err(self.err_here("expected `:`"));
                }
                let tok = self.next().map(|t| t.tok);
                let value = scalar_value(name, *s, tok)?;
                Ok(Field::new(id, s.wire_class(), value))
            }
        }
    }
}

fn parse_int(name: &str, text: &str) -> Result<i128, TextError> {
    let bad = || TextError::TypeMismatch {
        field: name.to_string(),
        expected: "integer",
    };
    let (neg, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let v = if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        i128::from_str_radix(hex, 16).map_err(|_| bad())?
    } else {
        if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        body.parse::<i128>()
            .map_err(|_| TextError::OutOfRange(name.to_string()))?
    };
    Ok(if neg { -v } else { v })
}

fn scalar_value(name: &str, s: Scalar, tok: Option<Tok>) -> Result<Value, TextError> {
    let mismatch = |expected: &'static str| TextError::TypeMismatch {
        field: name.to_string(),
        expected,
    };
    let range = || TextError::OutOfRange(name.to_string());
    match s {
        Scalar::String | Scalar::Bytes => {
            let Some(Tok::Str(bytes)) = tok else {
                return Err(mismatch("quoted string"));
            };
            if s == Scalar::String && std::str::from_utf8(&bytes).is_err() {
                return Err(TextError::InvalidUtf8(name.to_string()));
            }
            Ok(Value::Len(bytes))
        }
        Scalar::Bool => match tok {
            Some(Tok::Ident(v)) if v == "true" => Ok(Value::Varint(1)),
            Some(Tok::Ident(v)) if v == "false" => Ok(Value::Varint(0)),
            _ => Err(mismatch("`true` or `false`")),
        },
        Scalar::Float32 | Scalar::Float64 => {
            let text = match tok {
                Some(Tok::Num(t)) => t,
                Some(Tok::Ident(t)) => t,
                _ => return Err(mismatch("number")),
            };
            let lower = text.to_ascii_lowercase();
            let (neg, body) = match lower.strip_prefix('-') {
                Some(r) => (true, r.to_string()),
                None => (false, lower.strip_prefix('+').unwrap_or(&lower).to_string()),
            };
            let special = match body.as_str() {
                "nan" => Some(f64::NAN),
                "inf" | "infinity" => Some(f64::INFINITY),
                _ => None,
            };
            if s == Scalar::Float32 {
                let v: f32 = match special {
                    Some(v) => v as f32,
                    None => body.parse::<f32>().map_err(|_| mismatch("number"))?,
                };
                let v = if neg { -v } else { v };
                Ok(Value::Fixed32(v.to_bits()))
            } else {
                let v: f64 = match special {
                    Some(v) => v,
                    None => body.parse::<f64>().map_err(|_| mismatch("number"))?,
                };
                let v = if neg { -v } else { v };
                Ok(Value::Fixed64(v.to_bits()))
            }
        }
        _ => {
            let Some(Tok::Num(text)) = tok else {
                return Err(mismatch("integer"));
            };
            let n = parse_int(name, &text)?;
            match s {
                Scalar::Int32 => Ok(Value::Varint(
                    i64::from(i32::try_from(n).map_err(|_| range())?) as u64,
                )),
                Scalar::Int64 => Ok(Value::Varint(i64::try_from(n).map_err(|_| range())? as u64)),
                Scalar::Uint32 => Ok(Value::Varint(u64::from(
                    u32::try_from(n).map_err(|_| range())?,
                ))),
                Scalar::Uint64 => Ok(Value::Varint(u64::try_from(n).map_err(|_| range())?)),
                Scalar::Sint32 => Ok(Value::Varint(encode_zigzag(i64::from(
                    i32::try_from(n).map_err(|_| range())?,
                )))),
                Scalar::Sint64 => Ok(Value::Varint(encode_zigzag(
                    i64::try_from(n).map_err(|_| range())?,
                ))),
                Scalar::Fixed32 => Ok(Value::Fixed32(u32::try_from(n).map_err(|_| range())?)),
                Scalar::Sfixed32 => {
                    Ok(Value::Fixed32(i32::try_from(n).map_err(|_| range())? as u32))
                }
                Scalar::Fixed64 => Ok(Value::Fixed64(u64::try_from(n).map_err(|_| range())?)),
                Scalar::Sfixed64 => {
                    Ok(Value::Fixed64(i64::try_from(n).map_err(|_| range())? as u64))
                }
                _ => unreachable!("handled above"),
            }
        }
    }
}

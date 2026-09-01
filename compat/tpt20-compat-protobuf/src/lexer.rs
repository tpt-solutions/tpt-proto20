//! Lexer for `.proto` schema files.
//!
//! Produces a stream of `Token` values from raw input text. Handles proto2,
//! proto3, and Editions syntax declarations.

use crate::error::ProtoError;
use crate::proto_ast::{Span, Token, TokenKind};

/// Lexes `input` into a vector of tokens.
pub fn lex(input: &str) -> Result<Vec<Token>, crate::error::ProtoError> {
    let lexer = Lexer::new(input);
    lexer.lex()
}

struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Lexer {
            input: input.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn bump(&mut self) -> Option<u8> {
        if self.pos < self.input.len() {
            let b = self.input[self.pos];
            self.pos += 1;
            if b == b'\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            Some(b)
        } else {
            None
        }
    }

    fn span(&self) -> Span {
        Span {
            line: self.line,
            column: self.column,
        }
    }

    fn skip_ws_and_comments(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    self.bump();
                }
                b'/' => {
                    if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == b'/' {
                        while self.peek() != Some(b'\n') && self.peek().is_some() {
                            self.bump();
                        }
                    } else if self.pos + 1 < self.input.len() && self.input[self.pos + 1] == b'*' {
                        self.bump(); // /
                        self.bump(); // *
                        let mut closed = false;
                        while self.peek().is_some() {
                            if self.input[self.pos] == b'*'
                                && self.pos + 1 < self.input.len()
                                && self.input[self.pos + 1] == b'/'
                            {
                                self.bump();
                                self.bump();
                                closed = true;
                                break;
                            }
                            self.bump();
                        }
                        if !closed {
                            // We just consume to EOF; no hard error needed.
                        }
                    } else {
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    fn lex(mut self) -> Result<Vec<Token>, crate::error::ProtoError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_ws_and_comments();
            let start_span = self.span();
            match self.peek() {
                None => {
                    tokens.push(Token::new(TokenKind::Eof, start_span));
                    return Ok(tokens);
                }
                Some(b) => match b {
                    b';' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::Semi, start_span));
                    }
                    b'{' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::LBrace, start_span));
                    }
                    b'}' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::RBrace, start_span));
                    }
                    b'(' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::LParen, start_span));
                    }
                    b')' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::RParen, start_span));
                    }
                    b'<' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::LAngle, start_span));
                    }
                    b'>' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::RAngle, start_span));
                    }
                    b',' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::Comma, start_span));
                    }
                    b'=' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::Eq, start_span));
                    }
                    b'.' => {
                        self.bump();
                        tokens.push(Token::new(TokenKind::Dot, start_span));
                    }
                    b'"' | b'\'' => {
                        let s = self.lex_string(b)?;
                        tokens.push(Token::new(TokenKind::StringLit(s), start_span));
                    }
                    _ if b.is_ascii_digit() || (b == b'-' && self.peek_at(1).is_some() && self.peek_at(1).unwrap().is_ascii_digit()) => {
                        let n = self.lex_number()?;
                        tokens.push(Token::new(n, start_span));
                    }
                    _ if b.is_ascii_alphabetic() || b == b'_' => {
                        let ident = self.lex_ident();
                        let kind = match ident.as_str() {
                            "syntax" => TokenKind::Syntax,
                            "import" => TokenKind::Import,
                            "public" => TokenKind::Public,
                            "weak" => TokenKind::Weak,
                            "option" => TokenKind::Option,
                            "package" => TokenKind::Package,
                            "message" => TokenKind::Message,
                            "enum" => TokenKind::Enum,
                            "oneof" => TokenKind::Oneof,
                            "map" => TokenKind::Map,
                            "reserved" => TokenKind::Reserved,
                            "extend" => TokenKind::Extend,
                            "service" => TokenKind::Service,
                            "rpc" => TokenKind::Rpc,
                            "returns" => TokenKind::Returns,
                            "stream" => TokenKind::Stream,
                            "optional" => TokenKind::Optional,
                            "repeated" => TokenKind::Repeated,
                            "required" => TokenKind::Required,
                            "default" => TokenKind::Default,
                            "max" => TokenKind::Max,
                            "deprecated" => TokenKind::Deprecated,
                            "packed" => TokenKind::Packed,
                            "float" => TokenKind::Float,
                            "double" => TokenKind::Double,
                            "int32" => TokenKind::Int32,
                            "int64" => TokenKind::Int64,
                            "uint32" => TokenKind::UInt32,
                            "uint64" => TokenKind::UInt64,
                            "sint32" => TokenKind::SInt32,
                            "sint64" => TokenKind::SInt64,
                            "fixed32" => TokenKind::Fixed32,
                            "fixed64" => TokenKind::Fixed64,
                            "sfixed32" => TokenKind::SFixed32,
                            "sfixed64" => TokenKind::SFixed64,
                            "bool" => TokenKind::Bool,
                            "string" => TokenKind::String,
                            "bytes" => TokenKind::Bytes,
                            "true" => TokenKind::True,
                            "false" => TokenKind::False,
                            _ => TokenKind::Ident(ident),
                        };
                        tokens.push(Token::new(kind, start_span));
                    }
                    _ => {
                        let ch = self.bump().unwrap();
                        return Err(ProtoError::Lex {
                            line: start_span.line,
                            column: start_span.column,
                            message: format!("unexpected character '{}'", ch as char),
                        });
                    }
                },
            }
        }
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        let idx = self.pos + offset;
        if idx < self.input.len() {
            Some(self.input[idx])
        } else {
            None
        }
    }

    fn lex_string(&mut self, quote: u8) -> Result<String, crate::error::ProtoError> {
        let start = self.span();
        self.bump(); // consume opening quote
        let mut s = String::new();
        while let Some(b) = self.peek() {
            if b == quote {
                self.bump();
                return Ok(s);
            }
            if b == b'\\' {
                self.bump();
                let esc = match self.bump() {
                    Some(b'n') => '\n',
                    Some(b't') => '\t',
                    Some(b'r') => '\r',
                    Some(b'\\') => '\\',
                    Some(b'\'') => '\'',
                    Some(b'"') => '"',
                    Some(b'a') => '\x07',
                    Some(b'b') => '\x08',
                    Some(b'f') => '\x0c',
                    Some(b'v') => '\x0b',
                    Some(b'0') => '\0',
                    Some(b'x') => {
                        let h1 = self.bump().ok_or_else(|| ProtoError::Lex {
                            line: self.line,
                            column: self.column,
                            message: "incomplete hex escape".into(),
                        })?;
                        let h2 = self.bump().ok_or_else(|| ProtoError::Lex {
                            line: self.line,
                            column: self.column,
                            message: "incomplete hex escape".into(),
                        })?;
                        let v = hex_val(h1)?;
                        let w = hex_val(h2)?;
                        char::from_u32((v << 4) | w).unwrap_or('\0')
                    }
                    Some(b) => {
                        return Err(ProtoError::Lex {
                            line: start.line,
                            column: start.column,
                            message: format!("unknown escape '\\{}'", b as char),
                        });
                    }
                    None => {
                        return Err(ProtoError::Lex {
                            line: start.line,
                            column: start.column,
                            message: "unterminated escape".into(),
                        });
                    }
                };
                s.push(esc);
            } else if b == b'\n' {
                return Err(ProtoError::Lex {
                    line: start.line,
                    column: start.column,
                    message: "unterminated string literal".into(),
                });
            } else {
                let ch = self.bump().unwrap();
                s.push(ch as char);
            }
        }
        Err(ProtoError::Lex {
            line: start.line,
            column: start.column,
            message: "unterminated string literal".into(),
        })
    }

    fn lex_number(&mut self) -> Result<TokenKind, crate::error::ProtoError> {
        let mut s = String::new();
        if self.peek() == Some(b'-') {
            s.push('-');
            self.bump();
        }
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() || b.is_ascii_hexdigit() || b == b'x' || b == b'X' || b == b'.' || b == b'e' || b == b'E' || b == b'+' || b == b'-' {
                s.push(b as char);
                self.bump();
            } else {
                break;
            }
        }
        if s.contains('.') || s.contains('e') || s.contains('E') {
            Ok(TokenKind::FloatLit(s))
        } else if s.starts_with("0x") || s.starts_with("0X") {
            let val = i64::from_str_radix(&s[2..], 16).map_err(|_| ProtoError::Lex {
                line: self.line,
                column: self.column,
                message: format!("invalid hex integer '{}'", s),
            })?;
            Ok(TokenKind::IntLit(val))
        } else {
            let val = s.parse::<i64>().map_err(|_| ProtoError::Lex {
                line: self.line,
                column: self.column,
                message: format!("invalid integer '{}'", s),
            })?;
            Ok(TokenKind::IntLit(val))
        }
    }

    fn lex_ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' {
                s.push(b as char);
                self.bump();
            } else {
                break;
            }
        }
        s
    }
}

fn hex_val(b: u8) -> Result<u32, crate::error::ProtoError> {
    match b {
        b'0'..=b'9' => Ok((b - b'0') as u32),
        b'a'..=b'f' => Ok((b - b'a' + 10) as u32),
        b'A'..=b'F' => Ok((b - b'A' + 10) as u32),
        _ => Err(ProtoError::Lex {
            line: 0,
            column: 0,
            message: format!("invalid hex digit '{}'", b as char),
        }),
    }
}

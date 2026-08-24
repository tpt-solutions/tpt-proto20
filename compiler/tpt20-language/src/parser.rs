//! Recursive-descent parser for the tpt20 schema language (spec §6).
//!
//! Produces an [`ast::File`] from a token stream. The parser rejects the
//! `required` keyword (spec §6) and tracks spans for diagnostics (Phase 2).

use crate::ast::*;
use crate::lexer::{LexError, Span, SpannedToken, Token};

/// Errors produced by the parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A lexing error occurred before parsing.
    Lex(LexError),
    /// An unexpected token was found.
    UnexpectedToken {
        /// The unexpected token (debug-formatted).
        found: String,
        /// Where it occurred.
        at: Span,
    },
    /// End of input was reached unexpectedly.
    UnexpectedEof,
    /// The `required` keyword is not part of the language.
    RequiredNotAllowed(Span),
    /// A field id was missing or malformed.
    MissingFieldId(Span),
    /// A bare type name was expected.
    ExpectedType(Span),
    /// A numeric literal was expected.
    ExpectedNumber(Span),
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError::Lex(e)
    }
}

struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].token
    }

    fn span(&self) -> Span {
        self.tokens[self.pos.min(self.tokens.len() - 1)].span
    }

    fn bump(&mut self) -> SpannedToken {
        let t = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if self.peek() == expected {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        if self.peek() == expected {
            self.bump();
            Ok(())
        } else {
            Err(ParseError::UnexpectedToken {
                found: format!("{:?}", self.peek()),
                at: self.span(),
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.bump();
                Ok(s)
            }
            _ => Err(ParseError::UnexpectedToken {
                found: format!("{:?}", self.peek()),
                at: self.span(),
            }),
        }
    }

    fn parse_file(&mut self) -> Result<File, ParseError> {
        let mut file = File::default();
        while self.peek() != &Token::Eof {
            match self.peek().clone() {
                Token::Package => {
                    self.bump();
                    file.package = Some(self.parse_dotted_name()?);
                    self.expect(&Token::Semi)?;
                }
                Token::Import => {
                    self.bump();
                    match self.bump().token {
                        Token::String(s) => file.imports.push(s),
                        other => {
                            return Err(ParseError::UnexpectedToken {
                                found: format!("{other:?}"),
                                at: self.span(),
                            })
                        }
                    }
                    self.expect(&Token::Semi)?;
                }
                Token::Message => {
                    file.messages.push(self.parse_message()?);
                }
                Token::Enum | Token::Open | Token::Closed => file.enums.push(self.parse_enum()?),
                Token::Service => file.services.push(self.parse_service()?),
                Token::Reserved => file.reserved.push(self.parse_reserved()?),
                other => {
                    return Err(ParseError::UnexpectedToken {
                        found: format!("{other:?}"),
                        at: self.span(),
                    })
                }
            }
        }
        Ok(file)
    }

    fn parse_dotted_name(&mut self) -> Result<String, ParseError> {
        let first = self.expect_ident()?;
        let mut name = first;
        while self.peek() == &Token::Dot {
            self.bump();
            name.push('.');
            name.push_str(&self.expect_ident()?);
        }
        Ok(name)
    }

    fn parse_message(&mut self) -> Result<Message, ParseError> {
        self.expect(&Token::Message)?;
        let span = self.span();
        let name = self.expect_ident()?;
        let annotations = self.parse_annotations()?;
        self.expect(&Token::BraceOpen)?;
        let mut msg = Message {
            name,
            fields: Vec::new(),
            oneofs: Vec::new(),
            messages: Vec::new(),
            enums: Vec::new(),
            reserved: Vec::new(),
            annotations,
            span,
        };
        while self.peek() != &Token::BraceClose {
            match self.peek().clone() {
                Token::Message => msg.messages.push(self.parse_message()?),
                Token::Enum | Token::Open | Token::Closed => msg.enums.push(self.parse_enum()?),
                Token::Oneof => msg.oneofs.push(self.parse_oneof()?),
                Token::Reserved => msg.reserved.push(self.parse_reserved()?),
                Token::Required => {
                    return Err(ParseError::RequiredNotAllowed(self.span()));
                }
                _ => {
                    let leading = self.parse_annotations()?;
                    let (field, _) = self.parse_field(leading)?;
                    msg.fields.push(field);
                }
            }
        }
        self.expect(&Token::BraceClose)?;
        Ok(msg)
    }

    fn parse_oneof(&mut self) -> Result<Oneof, ParseError> {
        self.expect(&Token::Oneof)?;
        let span = self.span();
        let name = self.expect_ident()?;
        let annotations = self.parse_annotations()?;
        self.expect(&Token::BraceOpen)?;
        let mut oneof = Oneof {
            name,
            fields: Vec::new(),
            annotations,
            span,
        };
        while self.peek() != &Token::BraceClose {
            let leading = self.parse_annotations()?;
            let (field, _) = self.parse_field(leading)?;
            oneof.fields.push(field);
        }
        self.expect(&Token::BraceClose)?;
        Ok(oneof)
    }

    fn parse_enum(&mut self) -> Result<Enum, ParseError> {
        // Optional leading open/closed modifier (e.g. `open enum ...`).
        let mut open = false;
        if self.peek() == &Token::Open {
            self.bump();
            open = true;
        } else if self.peek() == &Token::Closed {
            self.bump();
            open = false;
        }
        self.expect(&Token::Enum)?;
        let span = self.span();
        let name = self.expect_ident()?;
        // Optional trailing open/closed modifier overrides the leading one.
        if self.peek() == &Token::Open {
            self.bump();
            open = true;
        } else if self.peek() == &Token::Closed {
            self.bump();
            open = false;
        }
        let annotations = self.parse_annotations()?;
        self.expect(&Token::BraceOpen)?;
        let mut en = Enum {
            name,
            values: Vec::new(),
            open,
            annotations,
            span,
        };
        while self.peek() != &Token::BraceClose {
            let v_span = self.span();
            let value_name = self.expect_ident()?;
            let mut alias = false;
            if self.peek() == &Token::Alias {
                self.bump();
                alias = true;
            }
            self.expect(&Token::Equals)?;
            let number = self.parse_number()?;
            let value_annotations = self.parse_annotations()?;
            let _ = value_annotations;
            self.expect(&Token::Semi)?;
            en.values.push(EnumValue {
                name: value_name,
                number: number as i32,
                alias,
                span: v_span,
            });
        }
        self.expect(&Token::BraceClose)?;
        Ok(en)
    }

    fn parse_service(&mut self) -> Result<Service, ParseError> {
        self.expect(&Token::Service)?;
        let span = self.span();
        let name = self.expect_ident()?;
        let annotations = self.parse_annotations()?;
        self.expect(&Token::BraceOpen)?;
        let mut service = Service {
            name,
            methods: Vec::new(),
            annotations,
            span,
        };
        while self.peek() != &Token::BraceClose {
            service.methods.push(self.parse_method()?);
        }
        self.expect(&Token::BraceClose)?;
        Ok(service)
    }

    fn parse_method(&mut self) -> Result<Method, ParseError> {
        let span = self.span();
        let name = self.expect_ident()?;
        self.expect(&Token::ParenOpen)?;
        let request_streaming = self.eat(&Token::Stream);
        let request = self.parse_type()?;
        self.expect(&Token::ParenClose)?;
        // `returns` keyword separates request and response.
        match self.bump().token {
            Token::Ident(s) if s == "returns" => {}
            other => {
                return Err(ParseError::UnexpectedToken {
                    found: format!("{other:?}"),
                    at: self.span(),
                })
            }
        }
        self.expect(&Token::ParenOpen)?;
        let response_streaming = self.eat(&Token::Stream);
        let response = self.parse_type()?;
        self.expect(&Token::ParenClose)?;
        let annotations = self.parse_annotations()?;
        self.expect(&Token::Semi)?;
        Ok(Method {
            name,
            request,
            request_streaming,
            response,
            response_streaming,
            annotations,
            span,
        })
    }

    /// Parses a field: `[(@ann)*] id : [repeated] [map<K,V>] name [?] (@ann)* ;`
    /// (name precedes type, matching the spec §6.1 example grammar).
    /// Annotations may appear before the id or trailing after the presence
    /// marker (spec §6.9, e.g. `1: email string? @max_len(254);`); both spellings
    /// attach to this field.
    fn parse_field(&mut self, leading: Vec<Annotation>) -> Result<(Field, ()), ParseError> {
        let id_span = self.span();
        let id = self
            .parse_number()
            .map_err(|_| ParseError::MissingFieldId(id_span))?;
        self.expect(&Token::Colon)?;

        if self.peek() == &Token::Required {
            return Err(ParseError::RequiredNotAllowed(self.span()));
        }

        let mut repeated = false;
        if self.eat(&Token::Repeated) {
            repeated = true;
        }

        let (label, field_name) = if self.peek() == &Token::Map {
            self.bump();
            self.expect(&Token::AngleOpen)?;
            let key = self.parse_type()?;
            self.expect(&Token::Comma)?;
            let value = self.parse_type()?;
            self.expect(&Token::AngleClose)?;
            let name = self.expect_ident()?;
            (FieldLabel::Map { key, value }, name)
        } else {
            let field_name = self.expect_ident()?;
            let ty = self.parse_type()?;
            let label = if repeated {
                FieldLabel::Repeated(ty)
            } else {
                FieldLabel::Singular(ty)
            };
            (label, field_name)
        };

        let presence = if self.eat(&Token::Question) {
            Presence::Explicit
        } else {
            Presence::Implicit
        };

        // Trailing annotations bind to this field (spec §6.9).
        let mut annotations = leading;
        annotations.extend(self.parse_annotations()?);
        self.expect(&Token::Semi)?;

        Ok((
            Field {
                id: id as u32,
                name: field_name,
                label,
                presence,
                annotations,
                span: id_span,
            },
            (),
        ))
    }

    fn parse_reserved(&mut self) -> Result<Reserved, ParseError> {
        self.expect(&Token::Reserved)?;
        let mut ids = Vec::new();
        let mut names = Vec::new();
        loop {
            match self.peek().clone() {
                Token::Int(n) => {
                    self.bump();
                    if self.eat(&Token::Range) {
                        let hi = self.parse_number()?;
                        ids.push(ReservedId::Range(n as u32, hi as u32));
                    } else if matches!(self.peek(), Token::Ident(s) if s == "to") {
                        self.bump();
                        let hi = self.parse_number()?;
                        ids.push(ReservedId::Range(n as u32, hi as u32));
                    } else {
                        ids.push(ReservedId::Single(n as u32));
                    }
                }
                Token::String(s) => {
                    self.bump();
                    names.push(s);
                }
                other => {
                    return Err(ParseError::UnexpectedToken {
                        found: format!("{other:?}"),
                        at: self.span(),
                    })
                }
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::Semi)?;
        Ok(Reserved { ids, names })
    }

    fn parse_type(&mut self) -> Result<TypeRef, ParseError> {
        let span = self.span();
        let first = self
            .expect_ident()
            .map_err(|_| ParseError::ExpectedType(span))?;
        let mut path = vec![first];
        while self.peek() == &Token::Dot {
            self.bump();
            path.push(self.expect_ident()?);
        }
        Ok(TypeRef { path })
    }

    fn parse_number(&mut self) -> Result<i64, ParseError> {
        match self.bump().token {
            Token::Int(n) => Ok(n),
            _ => Err(ParseError::ExpectedNumber(self.span())),
        }
    }

    fn parse_annotations(&mut self) -> Result<Vec<Annotation>, ParseError> {
        let mut annotations = Vec::new();
        while self.peek() == &Token::At {
            self.bump();
            let name = self.expect_ident()?;
            let mut args = Vec::new();
            if self.eat(&Token::ParenOpen) {
                if self.peek() != &Token::ParenClose {
                    loop {
                        args.push(self.parse_annotation_arg()?);
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&Token::ParenClose)?;
            }
            annotations.push(Annotation { name, args });
        }
        Ok(annotations)
    }

    fn parse_annotation_arg(&mut self) -> Result<AnnotationArg, ParseError> {
        match self.peek().clone() {
            Token::String(s) => {
                self.bump();
                Ok(AnnotationArg::String(s))
            }
            Token::Int(n) => {
                self.bump();
                Ok(AnnotationArg::Int(n))
            }
            Token::Ident(s) => {
                self.bump();
                match s.as_str() {
                    "true" => Ok(AnnotationArg::Bool(true)),
                    "false" => Ok(AnnotationArg::Bool(false)),
                    _ => Ok(AnnotationArg::Ident(s)),
                }
            }
            other => Err(ParseError::UnexpectedToken {
                found: format!("{other:?}"),
                at: self.span(),
            }),
        }
    }
}

/// Parses `.tpt` source into an [`ast::File`].
pub fn parse(src: &str) -> Result<File, ParseError> {
    let tokens = crate::lexer::lex(src)?;
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_file()
}

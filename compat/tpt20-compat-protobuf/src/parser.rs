//! Recursive-descent parser for `.proto` schema files (spec §10.1).
//!
//! Parses a token stream into a [`ProtoFile`] AST. Supports proto2, proto3,
//! and Editions syntax declarations.

use crate::error::ProtoError;
use crate::proto_ast::*;

/// Parses a token stream into a [`ProtoFile`].
pub fn parse(tokens: Vec<Token>) -> Result<ProtoFile, ProtoError> {
    let mut p = Parser::new(tokens);
    p.parse_file()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].kind
    }

    fn span(&self) -> Span {
        self.tokens[self.pos.min(self.tokens.len() - 1)].span
    }

    fn bump(&mut self) -> TokenKind {
        let t = self.tokens[self.pos.min(self.tokens.len() - 1)].kind.clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, expected: &TokenKind) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &TokenKind) -> Result<(), ProtoError> {
        if self.peek() == expected {
            self.bump();
            Ok(())
        } else {
            Err(ProtoError::UnexpectedToken {
                found: format!("{:?}", self.peek()),
                line: self.span().line,
                column: self.span().column,
                expected: "expected token",
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String, ProtoError> {
        match self.bump() {
            TokenKind::Ident(s) => Ok(s),
            other => Err(ProtoError::UnexpectedToken {
                found: format!("{:?}", other),
                line: self.span().line,
                column: self.span().column,
                expected: "identifier",
            }),
        }
    }

    fn parse_file(&mut self) -> Result<ProtoFile, ProtoError> {
        let mut file = ProtoFile::default();
        while *self.peek() != TokenKind::Eof {
            match self.peek() {
                TokenKind::Syntax => self.parse_syntax(&mut file)?,
                TokenKind::Import => self.parse_import(&mut file)?,
                TokenKind::Package => self.parse_package(&mut file)?,
                TokenKind::Option => self.parse_option_top(&mut file)?,
                TokenKind::Message => self.parse_message(&mut file.messages)?,
                TokenKind::Enum => self.parse_enum(&mut file.enums)?,
                TokenKind::Service => self.parse_service(&mut file.services)?,
                TokenKind::Extend => self.parse_extend(&mut file.extensions)?,
                TokenKind::Reserved => {
                    let r = self.parse_reserved()?;
                    file.reserved.push(r);
                }
                other => {
                    return Err(ProtoError::UnexpectedToken {
                        found: format!("{:?}", other),
                        line: self.span().line,
                        column: self.span().column,
                        expected: "top-level declaration",
                    });
                }
            }
        }
        Ok(file)
    }

    fn parse_syntax(&mut self, file: &mut ProtoFile) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Syntax)?;
        self.expect(&TokenKind::Eq)?;
        let s = self.expect_string_lit()?;
        self.expect(&TokenKind::Semi)?;
        file.syntax = Some(s);
        Ok(())
    }

    fn parse_import(&mut self, file: &mut ProtoFile) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Import)?;
        let mut public = false;
        let mut weak = false;
        if self.eat(&TokenKind::Public) {
            public = true;
        } else if self.eat(&TokenKind::Weak) {
            weak = true;
        }
        let path = self.expect_string_lit()?;
        self.expect(&TokenKind::Semi)?;
        file.imports.push(Import { public, weak, path });
        Ok(())
    }

    fn parse_package(&mut self, file: &mut ProtoFile) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Package)?;
        let mut name = self.expect_ident()?;
        while self.eat(&TokenKind::Dot) {
            name.push('.');
            name.push_str(&self.expect_ident()?);
        }
        self.expect(&TokenKind::Semi)?;
        file.package = Some(name);
        Ok(())
    }

    fn parse_option_top(&mut self, file: &mut ProtoFile) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Option)?;
        let name = self.parse_qualified_ident()?;
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_option_value()?;
        self.expect(&TokenKind::Semi)?;
        file.options.push(OptionDecl { name, value });
        Ok(())
    }

    fn parse_option_value(&mut self) -> Result<OptionValue, ProtoError> {
        match self.peek() {
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.bump();
                Ok(OptionValue::StringLit(s))
            }
            TokenKind::IntLit(n) => {
                let n = *n;
                self.bump();
                Ok(OptionValue::Int(n))
            }
            TokenKind::FloatLit(s) => {
                let s = s.clone();
                self.bump();
                Ok(OptionValue::Float(s))
            }
            TokenKind::True => {
                self.bump();
                Ok(OptionValue::Bool(true))
            }
            TokenKind::False => {
                self.bump();
                Ok(OptionValue::Bool(false))
            }
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.bump();
                Ok(OptionValue::Ident(s))
            }
            other => Err(ProtoError::UnexpectedToken {
                found: format!("{:?}", other),
                line: self.span().line,
                column: self.span().column,
                expected: "option value",
            }),
        }
    }

    fn parse_qualified_ident(&mut self) -> Result<String, ProtoError> {
        let mut ident = self.expect_ident()?;
        while self.eat(&TokenKind::Dot) {
            ident.push('.');
            ident.push_str(&self.expect_ident()?);
        }
        Ok(ident)
    }

    fn parse_message(
        &mut self,
        out: &mut Vec<Message>,
    ) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Message)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut msg = Message {
            name,
            fields: Vec::new(),
            oneofs: Vec::new(),
            messages: Vec::new(),
            enums: Vec::new(),
            extensions: Vec::new(),
            reserved: Vec::new(),
            options: Vec::new(),
        };
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::Eof {
            match self.peek() {
                TokenKind::Option => self.parse_message_option(&mut msg)?,
                TokenKind::Message => self.parse_message(&mut msg.messages)?,
                TokenKind::Enum => self.parse_enum(&mut msg.enums)?,
                TokenKind::Oneof => self.parse_oneof(&mut msg)?,
                TokenKind::Reserved => {
                    let r = self.parse_reserved()?;
                    msg.reserved.push(r);
                }
                TokenKind::Extend => self.parse_extend(&mut msg.extensions)?,
                TokenKind::Required | TokenKind::Optional | TokenKind::Repeated => {
                    self.parse_field(&mut msg.fields, false)?
                }
                _ if is_type_keyword(self.peek()) => self.parse_field(&mut msg.fields, false)?,
                other => {
                    return Err(ProtoError::UnexpectedToken {
                        found: format!("{:?}", other),
                        line: self.span().line,
                        column: self.span().column,
                        expected: "message body element",
                    });
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        out.push(msg);
        Ok(())
    }

    fn parse_message_option(&mut self, msg: &mut Message) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Option)?;
        let name = self.parse_qualified_ident()?;
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_option_value()?;
        self.expect(&TokenKind::Semi)?;
        msg.options.push(OptionDecl { name, value });
        Ok(())
    }

    fn parse_field(&mut self, out: &mut Vec<Field>, in_oneof: bool) -> Result<(), ProtoError> {
        let label = match self.peek() {
            TokenKind::Repeated => {
                self.bump();
                FieldLabel::Repeated
            }
            TokenKind::Optional => {
                self.bump();
                FieldLabel::Optional
            }
            TokenKind::Required => {
                self.bump();
                FieldLabel::Required
            }
            _ => {
                if in_oneof {
                    FieldLabel::Singular
                } else {
                    FieldLabel::Singular
                }
            }
        };

        let field_type = self.parse_type()?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Eq)?;
        let number = match self.bump() {
            TokenKind::IntLit(n) => n as u32,
            other => {
                return Err(ProtoError::UnexpectedToken {
                    found: format!("{:?}", other),
                    line: self.span().line,
                    column: self.span().column,
                    expected: "field number",
                });
            }
        };
        let options = self.parse_field_options()?;
        self.expect(&TokenKind::Semi)?;
        out.push(Field {
            label,
            field_type,
            name,
            number,
            options,
        });
        Ok(())
    }

    fn parse_type(&mut self) -> Result<ProtoType, ProtoError> {
        match self.peek() {
            TokenKind::Double => { self.bump(); Ok(ProtoType::Double) }
            TokenKind::Float => { self.bump(); Ok(ProtoType::Float) }
            TokenKind::Int32 => { self.bump(); Ok(ProtoType::Int32) }
            TokenKind::Int64 => { self.bump(); Ok(ProtoType::Int64) }
            TokenKind::UInt32 => { self.bump(); Ok(ProtoType::UInt32) }
            TokenKind::UInt64 => { self.bump(); Ok(ProtoType::UInt64) }
            TokenKind::SInt32 => { self.bump(); Ok(ProtoType::SInt32) }
            TokenKind::SInt64 => { self.bump(); Ok(ProtoType::SInt64) }
            TokenKind::Fixed32 => { self.bump(); Ok(ProtoType::Fixed32) }
            TokenKind::Fixed64 => { self.bump(); Ok(ProtoType::Fixed64) }
            TokenKind::SFixed32 => { self.bump(); Ok(ProtoType::SFixed32) }
            TokenKind::SFixed64 => { self.bump(); Ok(ProtoType::SFixed64) }
            TokenKind::Bool => { self.bump(); Ok(ProtoType::Bool) }
            TokenKind::String => { self.bump(); Ok(ProtoType::String) }
            TokenKind::Bytes => { self.bump(); Ok(ProtoType::Bytes) }
            TokenKind::Map => {
                self.bump();
                self.expect(&TokenKind::LAngle)?;
                let key = self.parse_type()?;
                self.expect(&TokenKind::Comma)?;
                let value = self.parse_type()?;
                self.expect(&TokenKind::RAngle)?;
                Ok(ProtoType::Map { key: Box::new(key), value: Box::new(value) })
            }
            TokenKind::Ident(_) => {
                let mut path = vec![self.expect_ident()?];
                while self.eat(&TokenKind::Dot) {
                    path.push(self.expect_ident()?);
                }
                // Could be message or enum; we record the path and resolve later.
                Ok(ProtoType::Message { name: path })
            }
            other => Err(ProtoError::UnexpectedToken {
                found: format!("{:?}", other),
                line: self.span().line,
                column: self.span().column,
                expected: "field type",
            }),
        }
    }

    fn parse_field_options(&mut self) -> Result<Vec<OptionDecl>, ProtoError> {
        let mut opts = Vec::new();
        if self.eat(&TokenKind::LBrace) {
            loop {
                if self.eat(&TokenKind::RBrace) {
                    break;
                }
                self.expect(&TokenKind::Option)?;
                let name = self.parse_qualified_ident()?;
                self.expect(&TokenKind::Eq)?;
                let value = self.parse_option_value()?;
                opts.push(OptionDecl { name, value });
                if self.eat(&TokenKind::Comma) {
                    // optional trailing comma
                }
                if *self.peek() == TokenKind::RBrace {
                    break;
                }
                if *self.peek() == TokenKind::Semi {
                    self.bump();
                }
            }
        }
        Ok(opts)
    }

    fn parse_oneof(&mut self, msg: &mut Message) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Oneof)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut oneof = Oneof {
            name,
            fields: Vec::new(),
            options: Vec::new(),
        };
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::Eof {
            match self.peek() {
                TokenKind::Option => {
                    self.expect(&TokenKind::Option)?;
                    let opt_name = self.parse_qualified_ident()?;
                    self.expect(&TokenKind::Eq)?;
                    let value = self.parse_option_value()?;
                    self.expect(&TokenKind::Semi)?;
                    oneof.options.push(OptionDecl { name: opt_name, value });
                }
                _ if is_type_keyword(self.peek()) => {
                    self.parse_field(&mut oneof.fields, true)?;
                }
                TokenKind::Ident(_) => {
                    self.parse_field(&mut oneof.fields, true)?;
                }
                other => {
                    return Err(ProtoError::UnexpectedToken {
                        found: format!("{:?}", other),
                        line: self.span().line,
                        column: self.span().column,
                        expected: "oneof body element",
                    });
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        msg.oneofs.push(oneof);
        Ok(())
    }

    fn parse_enum(
        &mut self,
        out: &mut Vec<Enum>,
    ) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Enum)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut en = Enum {
            name,
            values: Vec::new(),
            options: Vec::new(),
            allow_alias: false,
        };
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::Eof {
            match self.peek() {
                TokenKind::Option => {
                    self.expect(&TokenKind::Option)?;
                    let opt_name = self.parse_qualified_ident()?;
                    self.expect(&TokenKind::Eq)?;
                    let value = self.parse_option_value()?;
                    self.expect(&TokenKind::Semi)?;
                    en.options.push(OptionDecl { name: opt_name, value });
                }
                TokenKind::Reserved => {
                    let r = self.parse_reserved()?;
                    let _ = r; // reserved in enums is parsed but not stored in simple IR
                }
                _ => {
                    let value_name = self.expect_ident()?;
                    self.expect(&TokenKind::Eq)?;
                    let number = match self.bump() {
                        TokenKind::IntLit(n) => n as i32,
                        other => {
                            return Err(ProtoError::UnexpectedToken {
                                found: format!("{:?}", other),
                                line: self.span().line,
                                column: self.span().column,
                                expected: "enum value number",
                            });
                        }
                    };
                    let opts = self.parse_field_options()?;
                    self.expect(&TokenKind::Semi)?;
                    en.values.push(EnumValue {
                        name: value_name,
                        number,
                        options: opts,
                    });
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        out.push(en);
        Ok(())
    }

    fn parse_service(&mut self, out: &mut Vec<Service>) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Service)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut svc = Service {
            name,
            methods: Vec::new(),
        };
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::Eof {
            self.expect(&TokenKind::Rpc)?;
            let method_name = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            let request_type = self.parse_qualified_ident()?;
            let request_streaming = self.eat(&TokenKind::Stream);
            self.expect(&TokenKind::RParen)?;
            self.expect(&TokenKind::Returns)?;
            self.expect(&TokenKind::LParen)?;
            let response_type = self.parse_qualified_ident()?;
            let response_streaming = self.eat(&TokenKind::Stream);
            self.expect(&TokenKind::RParen)?;
            let options = self.parse_field_options()?;
            self.expect(&TokenKind::Semi)?;
            svc.methods.push(Method {
                name: method_name,
                request_type: vec![request_type],
                request_streaming,
                response_type: vec![response_type],
                response_streaming,
                options,
            });
        }
        self.expect(&TokenKind::RBrace)?;
        out.push(svc);
        Ok(())
    }

    fn parse_extend(&mut self, out: &mut Vec<Extend>) -> Result<(), ProtoError> {
        self.expect(&TokenKind::Extend)?;
        let message_type = self.parse_qualified_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let mut ext = Extend {
            message_type: vec![message_type],
            fields: Vec::new(),
        };
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::Eof {
            self.parse_field(&mut ext.fields, false)?;
        }
        self.expect(&TokenKind::RBrace)?;
        out.push(ext);
        Ok(())
    }

    fn parse_reserved(&mut self) -> Result<Reserved, ProtoError> {
        self.expect(&TokenKind::Reserved)?;
        let mut reserved = Reserved {
            ids: Vec::new(),
            names: Vec::new(),
        };
        loop {
            if *self.peek() == TokenKind::Semi || *self.peek() == TokenKind::RBrace || *self.peek() == TokenKind::Eof {
                break;
            }
            if let TokenKind::IntLit(n) = self.peek() {
                let start = *n as u32;
                self.bump();
                if self.eat(&TokenKind::To) {
                    let end = match self.bump() {
                        TokenKind::IntLit(n) => n as u32,
                        other => {
                            return Err(ProtoError::UnexpectedToken {
                                found: format!("{:?}", other),
                                line: self.span().line,
                                column: self.span().column,
                                expected: "reserved range end",
                            });
                        }
                    };
                    reserved.ids.push(ReservedId::Range(start, end));
                } else {
                    reserved.ids.push(ReservedId::Single(start));
                }
            } else if let TokenKind::StringLit(s) = self.peek() {
                reserved.names.push(s.clone());
                self.bump();
            } else {
                return Err(ProtoError::UnexpectedToken {
                    found: format!("{:?}", self.peek()),
                    line: self.span().line,
                    column: self.span().column,
                    expected: "reserved id or name",
                });
            }
            if self.eat(&TokenKind::Comma) {
                // trailing comma ok
            }
        }
        self.expect(&TokenKind::Semi)?;
        Ok(reserved)
    }

    fn expect_string_lit(&mut self) -> Result<String, ProtoError> {
        match self.bump() {
            TokenKind::StringLit(s) => Ok(s),
            other => Err(ProtoError::UnexpectedToken {
                found: format!("{:?}", other),
                line: self.span().line,
                column: self.span().column,
                expected: "string literal",
            }),
        }
    }
}

fn is_type_keyword(tok: &TokenKind) -> bool {
    matches!(
        tok,
        TokenKind::Double
            | TokenKind::Float
            | TokenKind::Int32
            | TokenKind::Int64
            | TokenKind::UInt32
            | TokenKind::UInt64
            | TokenKind::SInt32
            | TokenKind::SInt64
            | TokenKind::Fixed32
            | TokenKind::Fixed64
            | TokenKind::SFixed32
            | TokenKind::SFixed64
            | TokenKind::Bool
            | TokenKind::String
            | TokenKind::Bytes
            | TokenKind::Map
    )
}

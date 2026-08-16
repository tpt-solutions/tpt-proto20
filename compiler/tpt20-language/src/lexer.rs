//! Lexer / tokenizer for the tpt20 schema language (spec §6).
//!
//! Produces a stream of [`Token`]s with attached source positions so the
//! parser and the later diagnostics engine can report accurate spans.

/// A lexical token.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Package keyword.
    Package,
    /// Import keyword.
    Import,
    /// Message keyword.
    Message,
    /// Enum keyword.
    Enum,
    /// Oneof keyword.
    Oneof,
    /// Service keyword.
    Service,
    /// Repeated keyword.
    Repeated,
    /// Map keyword.
    Map,
    /// Reserved keyword.
    Reserved,
    /// Stream keyword.
    Stream,
    /// Open keyword.
    Open,
    /// Closed keyword.
    Closed,
    /// Alias keyword.
    Alias,
    /// Required keyword (rejected by the parser).
    Required,
    /// `@` annotation marker.
    At,
    /// `?` explicit-presence marker.
    Question,
    /// `:` field-id separator.
    Colon,
    /// `;` statement terminator.
    Semi,
    /// `,` separator.
    Comma,
    /// `.` member / dot access.
    Dot,
    /// `=` assignment.
    Equals,
    /// `{` brace open.
    BraceOpen,
    /// `}` brace close.
    BraceClose,
    /// `(` paren open.
    ParenOpen,
    /// `)` paren close.
    ParenClose,
    /// `<` angle open (for map `<K, V>`).
    AngleOpen,
    /// `>` angle close.
    AngleClose,
    /// `..` or `..=` range operator. (Encountered as two dots here.)
    Range,
    /// An identifier.
    Ident(String),
    /// A string literal (without quotes).
    String(String),
    /// An integer literal.
    Int(i64),
    /// End of input.
    Eof,
}

/// Source position (1-based line and column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
}

/// A token with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    /// The token kind.
    pub token: Token,
    /// Start position of the token.
    pub span: Span,
}

/// Errors produced by the lexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// An unexpected character was encountered.
    UnexpectedChar(char, Span),
    /// An unterminated string literal.
    UnterminatedString(Span),
}

/// Lexes `.tpt` source into a vector of spanned tokens.
pub fn lex(src: &str) -> Result<Vec<SpannedToken>, LexError> {
    let mut tokens = Vec::new();
    let mut chars = src.char_indices().peekable();
    let mut line = 1usize;
    let mut col = 1usize;

    macro_rules! span {
        () => {
            Span { line, column: col }
        };
    }

    while let Some(&(i, c)) = chars.peek() {
        let start = span!();
        match c {
            c if c.is_whitespace() => {
                if c == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                chars.next();
            }
            '/' => {
                // Only `//` line comments are supported.
                let next = src[i + c.len_utf8()..].chars().next();
                if next == Some('/') {
                    let rest = &src[i..];
                    let end = rest.find('\n').unwrap_or(rest.len());
                    let consumed = &rest[..end];
                    let count = consumed.chars().count();
                    for _ in 0..count {
                        chars.next();
                    }
                    col += count;
                } else {
                    return Err(LexError::UnexpectedChar(c, start));
                }
            }
            '0'..='9' => {
                let mut value: i64 = 0;
                let mut digits = 0;
                while let Some(&(_, d)) = chars.peek() {
                    if d.is_ascii_digit() {
                        value = value
                            .saturating_mul(10)
                            .saturating_add((d as i64) - ('0' as i64));
                        digits += 1;
                        col += 1;
                        chars.next();
                    } else {
                        break;
                    }
                }
                let _ = digits;
                tokens.push(SpannedToken {
                    token: Token::Int(value),
                    span: start,
                });
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                let mut closed = false;
                loop {
                    match chars.next() {
                        Some((_, '"')) => {
                            closed = true;
                            col += 1;
                            break;
                        }
                        Some((_, '\n')) => break,
                        Some((_, ch)) => {
                            s.push(ch);
                            col += 1;
                        }
                        None => break,
                    }
                }
                if !closed {
                    return Err(LexError::UnterminatedString(start));
                }
                tokens.push(SpannedToken {
                    token: Token::String(s),
                    span: start,
                });
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut ident = String::new();
                while let Some(&(_, d)) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' {
                        ident.push(d);
                        col += 1;
                        chars.next();
                    } else {
                        break;
                    }
                }
                let tok = keyword_or_ident(&ident);
                tokens.push(SpannedToken {
                    token: tok,
                    span: start,
                });
            }
            ':' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::Colon,
                    span: start,
                });
            }
            ';' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::Semi,
                    span: start,
                });
            }
            ',' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::Comma,
                    span: start,
                });
            }
            '.' => {
                col += 1;
                chars.next();
                // Handle `..` / `..=` as a range.
                if matches!(chars.peek(), Some(&(_, '.'))) {
                    chars.next();
                    col += 1;
                    if matches!(chars.peek(), Some(&(_, '='))) {
                        chars.next();
                        col += 1;
                    }
                    tokens.push(SpannedToken {
                        token: Token::Range,
                        span: start,
                    });
                } else {
                    tokens.push(SpannedToken {
                        token: Token::Dot,
                        span: start,
                    });
                }
            }
            '=' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::Equals,
                    span: start,
                });
            }
            '{' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::BraceOpen,
                    span: start,
                });
            }
            '}' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::BraceClose,
                    span: start,
                });
            }
            '(' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::ParenOpen,
                    span: start,
                });
            }
            ')' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::ParenClose,
                    span: start,
                });
            }
            '<' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::AngleOpen,
                    span: start,
                });
            }
            '>' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::AngleClose,
                    span: start,
                });
            }
            '@' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::At,
                    span: start,
                });
            }
            '?' => {
                col += 1;
                chars.next();
                tokens.push(SpannedToken {
                    token: Token::Question,
                    span: start,
                });
            }
            other => {
                return Err(LexError::UnexpectedChar(other, start));
            }
        }
        let _ = i;
    }
    tokens.push(SpannedToken {
        token: Token::Eof,
        span: span!(),
    });
    Ok(tokens)
}

fn keyword_or_ident(ident: &str) -> Token {
    match ident {
        "package" => Token::Package,
        "import" => Token::Import,
        "message" => Token::Message,
        "enum" => Token::Enum,
        "oneof" => Token::Oneof,
        "service" => Token::Service,
        "repeated" => Token::Repeated,
        "map" => Token::Map,
        "reserved" => Token::Reserved,
        "stream" => Token::Stream,
        "open" => Token::Open,
        "closed" => Token::Closed,
        "alias" => Token::Alias,
        "required" => Token::Required,
        _ => Token::Ident(ident.to_string()),
    }
}

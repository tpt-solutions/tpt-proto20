//! `tpt20-language`: lexer, parser, and AST for the tpt20 schema language.
//!
//! This crate implements Phase 1 of the project todo: the `.tpt` file grammar,
//! tokenizer, parser, and AST data structures. Semantic analysis (Phase 2) and
//! codegen (Phase 5) consume the [`ast`] produced here.

pub mod ast;
pub mod lexer;
pub mod parser;

pub use ast::*;
pub use lexer::{lex, LexError, Span, SpannedToken, Token};
pub use parser::{parse, ParseError};

/// Parses `.tpt` source into an [`ast::File`], returning a descriptive error on
/// failure.
pub fn parse_file(src: &str) -> Result<ast::File, ParseError> {
    parse(src)
}

impl FieldLabel {
    /// Returns the inner type of a singular/repeated/map label.
    pub fn unwrap_type(&self) -> &TypeRef {
        match self {
            FieldLabel::Singular(t) | FieldLabel::Repeated(t) => t,
            FieldLabel::Map { value, .. } => value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec §6.1 example schema, round-tripped through parse.
    const EXAMPLE: &str = r#"
        package user.v1;

        import "common.proto";

        // A user account.
        message User {
            1: id int64;
            2: name string;
            3: email string?;              // explicit presence
            4: repeated tag string;
            5: map<string, string> attributes;
            oneof contact {
                10: email_addr string;
                11: phone string;
            }
            7: Status status;
        }

        enum Status {
            UNKNOWN = 0;
            ACTIVE = 1;
            INACTIVE = 2;
        }

        open enum Feature {
            NONE = 0;
            BETA = 1;
        }

        message GetUserRequest {
            1: id int64;
        }

        service UserService {
            GetUser(GetUserRequest) returns (User);
            Subscribe(stream GetUserRequest) returns (stream User);
        }

        reserved 100 to 200;
        reserved "deprecated_field";
    "#;

    #[test]
    fn parses_example_schema() {
        let file = parse(EXAMPLE).expect("example schema should parse");
        assert_eq!(file.package.as_deref(), Some("user.v1"));
        assert_eq!(file.imports, vec!["common.proto"]);

        let user = file.messages.iter().find(|m| m.name == "User").unwrap();
        // Field 3 has explicit presence.
        let email = user.fields.iter().find(|f| f.name == "email").unwrap();
        assert_eq!(email.presence, Presence::Explicit);
        assert_eq!(email.id, 3);

        // Repeated field.
        let tags = user.fields.iter().find(|f| f.name == "tag").unwrap();
        assert!(matches!(tags.label, FieldLabel::Repeated(_)));
        assert_eq!(tags.label.clone().unwrap_type().name(), "string");

        // Map field.
        let attrs = user.fields.iter().find(|f| f.name == "attributes").unwrap();
        assert!(matches!(attrs.label, FieldLabel::Map { .. }));

        // Oneof.
        let contact = user.oneofs.iter().find(|o| o.name == "contact").unwrap();
        assert_eq!(contact.fields.len(), 2);

        // Enums: closed and open.
        let status = file.enums.iter().find(|e| e.name == "Status").unwrap();
        assert!(!status.open);
        assert_eq!(status.values.len(), 3);
        let feature = file.enums.iter().find(|e| e.name == "Feature").unwrap();
        assert!(feature.open);

        // Service with unary and bidirectional streaming.
        let svc = file
            .services
            .iter()
            .find(|s| s.name == "UserService")
            .unwrap();
        assert_eq!(svc.methods.len(), 2);
        let sub = svc.methods.iter().find(|m| m.name == "Subscribe").unwrap();
        assert!(sub.request_streaming && sub.response_streaming);

        // Top-level reserved declarations.
        assert_eq!(file.reserved.len(), 2);
        assert_eq!(file.reserved[0].ids, vec![ReservedId::Range(100, 200)]);
        assert_eq!(file.reserved[1].names, vec!["deprecated_field".to_string()]);
    }

    #[test]
    fn rejects_required_keyword() {
        let src = "message M { 1: required x int32; }";
        assert!(matches!(parse(src), Err(ParseError::RequiredNotAllowed(_))));
    }

    #[test]
    fn reserved_ids_and_names() {
        let file = parse("message M { reserved 100 to 200; reserved \"old\"; }").unwrap();
        let r = &file.messages[0].reserved;
        assert_eq!(r[0].ids, vec![ReservedId::Range(100, 200)]);
        assert_eq!(r[1].names, vec!["old".to_string()]);
    }
}

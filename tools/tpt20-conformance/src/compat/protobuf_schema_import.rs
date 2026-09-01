use tpt20_compat_protobuf::{lex_proto, parse_proto, lower};

#[test]
fn lex_proto_simple() {
    let src = "syntax = \"proto3\"; message Foo { int32 x = 1; }";
    let tokens = lex_proto(src).unwrap();
    assert!(!tokens.is_empty());
}

#[test]
fn parse_proto_message() {
    let src = "syntax = \"proto3\"; message Foo { int32 x = 1; string y = 2; }";
    let tokens = lex_proto(src).unwrap();
    let ast = parse_proto(tokens).unwrap();
    assert_eq!(ast.messages.len(), 1);
    assert_eq!(ast.messages[0].name, "Foo");
    assert_eq!(ast.messages[0].fields.len(), 2);
}

#[test]
fn parse_proto_enum() {
    let src = "enum State { UNKNOWN = 0; ACTIVE = 1; }";
    let tokens = lex_proto(src).unwrap();
    let ast = parse_proto(tokens).unwrap();
    assert_eq!(ast.enums.len(), 1);
    assert_eq!(ast.enums[0].name, "State");
    assert_eq!(ast.enums[0].values.len(), 2);
}

#[test]
fn lower_proto_to_ir() {
    let src = "syntax = \"proto3\"; message Foo { int32 x = 1; }";
    let tokens = lex_proto(src).unwrap();
    let ast = parse_proto(tokens).unwrap();
    let ir = lower(ast).unwrap();
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].name, "Foo");
}

#[test]
fn lower_proto_import() {
    let src = "syntax = \"proto3\"; import \"common.proto\"; message Bar {}";
    let tokens = lex_proto(src).unwrap();
    let ast = parse_proto(tokens).unwrap();
    let ir = lower(ast).unwrap();
    assert_eq!(ir.imports, vec!["common.proto"]);
}

#[test]
fn lower_proto_package() {
    let src = "syntax = \"proto3\"; package example.v1; message Baz {}";
    let tokens = lex_proto(src).unwrap();
    let ast = parse_proto(tokens).unwrap();
    let ir = lower(ast).unwrap();
    assert_eq!(ir.name, Some("example.v1".to_string()));
}

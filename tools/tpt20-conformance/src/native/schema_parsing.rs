use tpt20_language::parse_file;

#[test]
fn parse_simple_schema() {
    let src = r#"package "test.v1"
message User {
  id: int64
  name: string
}"#;
    let file = parse_file(src).unwrap();
    assert_eq!(file.package, Some("test.v1".to_string()));
    assert_eq!(file.messages.len(), 1);
    assert_eq!(file.messages[0].name, "User");
    assert_eq!(file.messages[0].fields.len(), 2);
}

#[test]
fn parse_message_with_oneof() {
    let src = r#"message Request {
  oneof payload {
    text: string
    bytes: bytes
  }
}"#;
    let file = parse_file(src).unwrap();
    assert_eq!(file.messages[0].oneofs.len(), 1);
    assert_eq!(file.messages[0].oneofs[0].name, "payload");
    assert_eq!(file.messages[0].oneofs[0].fields.len(), 2);
}

#[test]
fn parse_message_with_reserved() {
    let src = r#"message Foo {
  reserved 1, 5 to 10
  reserved "foo", "bar"
}"#;
    let file = parse_file(src).unwrap();
    assert_eq!(file.messages[0].reserved.len(), 2);
}

#[test]
fn parse_enum() {
    let src = r#"enum State {
  ACTIVE = 0
  INACTIVE = 1
}"#;
    let file = parse_file(src).unwrap();
    assert_eq!(file.enums.len(), 1);
    assert_eq!(file.enums[0].name, "State");
    assert_eq!(file.enums[0].values.len(), 2);
}

#[test]
fn parse_import() {
    let src = r#"import "other.tpt"
message Bar {}"#;
    let file = parse_file(src).unwrap();
    assert_eq!(file.imports, vec!["other.tpt"]);
}

#[test]
fn parse_repeated_field() {
    let src = r#"message Foo {
  tags: repeated string
}"#;
    let file = parse_file(src).unwrap();
    assert!(matches!(
        file.messages[0].fields[0].label,
        tpt20_language::ast::FieldLabel::Repeated(_)
    ));
}

#[test]
fn parse_map_field() {
    let src = r#"message Foo {
  meta: map<string, int64>
}"#;
    let file = parse_file(src).unwrap();
    assert!(matches!(
        file.messages[0].fields[0].label,
        tpt20_language::ast::FieldLabel::Map { .. }
    ));
}

#[test]
fn parse_service() {
    let src = r#"service UserService {
  rpc GetUser (GetUserRequest) returns (User)
}"#;
    let file = parse_file(src).unwrap();
    assert_eq!(file.services.len(), 1);
    assert_eq!(file.services[0].name, "UserService");
}

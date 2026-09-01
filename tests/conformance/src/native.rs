//! Native conformance integration tests.

use tpt20_core::{
    DecoderLimits, DynamicMessage, Field, FieldDescriptor, FieldKind, MessageDescriptor,
    RawMessage, ScalarKind, UnknownFieldPolicy, Value, WireClass,
};
use tpt20_compiler::pipeline::check as semantic_check;
use tpt20_language::parse;

#[test]
fn parse_and_semantic_check_valid_schema() {
    let src = r#"package "test.v1"
message User {
  id: int64
  name: string
}"#;
    let file = parse(src).unwrap();
    let diags = semantic_check(src, None);
    let errors: Vec<_> = diags.iter().filter(|d| d.severity == tpt20_compiler::diagnostics::Severity::Error).collect();
    assert!(errors.is_empty());
    assert_eq!(file.package, Some("test.v1".to_string()));
}

#[test]
fn wire_roundtrip_native() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    msg.push(Field::new(2, WireClass::Len, Value::Len(b"hello".to_vec())));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(msg.fields, back.fields);
}

#[test]
fn dynamic_message_encode_decode() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));

    let mut msg = DynamicMessage::with_descriptor(desc.clone());
    msg.set_varint_by_name("id", 7).unwrap();
    msg.set_string_by_name("name", "test").unwrap();

    let bytes = msg.encode().unwrap();
    let back = DynamicMessage::decode_descriptor(desc, &bytes, &DecoderLimits::default()).unwrap();
    assert_eq!(back.get_varint_by_name("id").unwrap(), Some(7));
    assert_eq!(back.get_string_by_name("name").unwrap(), Some("test"));
}

#[test]
fn json_roundtrip_dynamic_message() {
    use tpt20_stdlib::json::base64;
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_field(FieldDescriptor::new(2, "data", WireClass::Len, FieldKind::Scalar(ScalarKind::Bytes)));

    let mut msg = DynamicMessage::with_descriptor(desc.clone());
    msg.set_varint_by_name("id", 42).unwrap();
    msg.set_bytes_by_name("data", b"hello").unwrap();

    let json = msg.to_json().unwrap();
    let back = DynamicMessage::from_json(desc, &json).unwrap();
    assert_eq!(back.get_varint_by_name("id").unwrap(), Some(42));
    assert_eq!(back.get_bytes_by_name("data"), Some(b"hello"));
}

#[test]
fn canonical_encoding_deterministic() {
    let mut a = RawMessage::new();
    a.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
    a.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    let mut b = RawMessage::new();
    b.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    b.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
    assert_eq!(a.encode_canonical().unwrap(), b.encode_canonical().unwrap());
}

#[test]
fn text_format_output() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));
    let mut msg = DynamicMessage::with_descriptor(desc);
    msg.set_varint_by_name("id", 42).unwrap();
    msg.set_string_by_name("name", "Ada").unwrap();
    let text = msg.to_text();
    assert!(text.contains("id: 42"));
    assert!(text.contains("name: \"Ada\""));
}

#[test]
fn security_limits_message_size() {
    let bytes = vec![0u8; 64];
    let limits = DecoderLimits {
        max_message_bytes: 16,
        ..DecoderLimits::default()
    };
    assert_eq!(
        RawMessage::decode(&bytes, &limits, UnknownFieldPolicy::Preserve),
        Err(tpt20_core::DecodeError::LimitExceeded { limit: 16 })
    );
}

#[test]
fn deadline_not_expired() {
    use std::time::Duration;
    use tpt20_rpc::Deadline;
    let d = Deadline::from_now(Duration::from_secs(10));
    assert!(!d.is_expired());
}

#[test]
fn cancellation_token_propagates() {
    use tpt20_rpc::CancellationToken;
    let token = CancellationToken::new();
    let clone = token.clone();
    token.cancel();
    assert!(clone.is_cancelled());
}

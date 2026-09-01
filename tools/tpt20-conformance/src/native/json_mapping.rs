use tpt20_core::{Field, FieldDescriptor, FieldKind, MessageDescriptor, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_core::ScalarKind;

#[test]
fn json_roundtrip_simple() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));

    let mut msg = tpt20_core::DynamicMessage::with_descriptor(desc.clone());
    msg.set_varint_by_name("id", 42).unwrap();
    msg.set_string_by_name("name", "Ada").unwrap();

    let json = msg.to_json().unwrap();
    let back = tpt20_core::DynamicMessage::from_json(desc, &json).unwrap();
    assert_eq!(back.get_varint_by_name("id").unwrap(), Some(42));
    assert_eq!(back.get_string_by_name("name").unwrap(), Some("Ada"));
}

#[test]
fn json_emits_int64_as_string() {
    use tpt20_stdlib::json::i64_to_value;
    let v = i64_to_value(i64::MIN);
    assert_eq!(v, serde_json::Value::String(i64::MIN.to_string()));
}

#[test]
fn json_emits_u64_as_string() {
    use tpt20_stdlib::json::u64_to_value;
    let v = u64_to_value(u64::MAX);
    assert_eq!(v, serde_json::Value::String(u64::MAX.to_string()));
}

#[test]
fn json_as_i64_accepts_number_and_string() {
    use tpt20_stdlib::json::as_i64;
    assert_eq!(as_i64(&serde_json::json!(42)).unwrap(), 42);
    assert_eq!(as_i64(&serde_json::json!("42")).unwrap(), 42);
    assert!(as_i64(&serde_json::json!("nope")).is_err());
}

#[test]
fn json_as_u64_accepts_number_and_string() {
    use tpt20_stdlib::json::as_u64;
    assert_eq!(as_u64(&serde_json::json!(42)).unwrap(), 42);
    assert_eq!(as_u64(&serde_json::json!(u64::MAX.to_string())).unwrap(), u64::MAX);
}

#[test]
fn json_base64_roundtrip() {
    use tpt20_stdlib::json::base64;
    let data = b"hello world";
    let encoded = base64::encode(data);
    let decoded = base64::decode(&encoded).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn json_base64_rejects_bad_input() {
    use tpt20_stdlib::json::base64;
    assert!(base64::decode("A").is_err());
    assert!(base64::decode("AB@=").is_err());
}

#[test]
fn json_get_field_accepts_aliases() {
    use tpt20_stdlib::json::get_field;
    let obj = serde_json::json!({"userId": 1}).as_object().unwrap().clone();
    assert!(get_field(&obj, &["user_id", "userId"]).is_some());
    assert!(get_field(&obj, &["userid"]).is_none());
}

#[test]
fn dynamic_message_json_without_descriptor() {
    let mut msg = tpt20_core::DynamicMessage::new();
    msg.set_varint(1, 42);
    msg.set_bytes(2, b"test");
    let json = msg.to_json().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.get("1").unwrap().as_str(), Some("42"));
    assert!(parsed.get("2").unwrap().is_string());
}

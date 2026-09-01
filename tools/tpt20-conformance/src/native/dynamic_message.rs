use tpt20_core::{DecoderLimits, Field, FieldDescriptor, FieldKind, MessageDescriptor, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_core::DynamicMessage;
use tpt20_core::ScalarKind;

fn make_user_descriptor() -> MessageDescriptor {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));
    desc.add_field(FieldDescriptor::new(3, "email", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));
    desc.add_field(FieldDescriptor::new(4, "tags", WireClass::Len, FieldKind::Repeated { packed: true }));
    desc
}

#[test]
fn descriptor_driven_decode() {
    let desc = make_user_descriptor();
    let mut msg = DynamicMessage::with_descriptor(desc.clone());
    msg.set_varint_by_name("id", 42).unwrap();
    msg.set_string_by_name("name", "Ada").unwrap();

    let bytes = msg.encode().unwrap();
    let back = DynamicMessage::decode_descriptor(desc, &bytes, &DecoderLimits::default()).unwrap();
    assert_eq!(back.get_varint_by_name("id").unwrap(), Some(42));
    assert_eq!(back.get_string_by_name("name").unwrap(), Some("Ada"));
}

#[test]
fn encode_decode_with_descriptor_roundtrip() {
    let desc = make_user_descriptor();
    let mut msg = DynamicMessage::with_descriptor(desc.clone());
    msg.set_varint_by_name("id", 7).unwrap();
    msg.set_string_by_name("name", "test").unwrap();
    msg.set_bytes_by_name("tags", b"tag1").unwrap();

    let bytes = msg.encode().unwrap();
    let back = DynamicMessage::decode_descriptor(desc, &bytes, &DecoderLimits::default()).unwrap();
    assert_eq!(back.get_varint_by_name("id").unwrap(), Some(7));
    assert_eq!(back.get_string_by_name("name").unwrap(), Some("test"));
    assert_eq!(back.get_bytes_by_name("tags"), Some(b"tag1"));
}

#[test]
fn unknown_fields_via_descriptor() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    let mut msg = DynamicMessage::with_descriptor(desc);
    msg.set_varint(1, 42);
    msg.set_varint(99, 1);
    let unknown = msg.unknown_fields();
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].field_id, 99);
}

#[test]
fn field_access_by_name() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));
    let mut msg = DynamicMessage::with_descriptor(desc.clone());
    msg.set_varint_by_name("id", 7).unwrap();
    msg.set_string_by_name("name", "test").unwrap();
    assert!(msg.get_field_by_name("id").is_some());
    assert!(msg.get_field_by_name("name").is_some());
    assert!(msg.get_field_by_name("missing").is_none());
}

#[test]
fn remove_by_name() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));
    let mut msg = DynamicMessage::with_descriptor(desc);
    msg.set_varint_by_name("id", 1).unwrap();
    msg.set_string_by_name("name", "test").unwrap();
    assert!(msg.remove_by_name("name"));
    assert!(msg.get_first(2).is_none());
    assert!(!msg.remove_by_name("missing"));
}

#[test]
fn encode_canonical_is_deterministic() {
    let desc = make_user_descriptor();
    let mut msg = DynamicMessage::decode_descriptor(desc.clone(), &[], &DecoderLimits::default()).unwrap();
    msg.set_varint_by_name("id", 1).unwrap();
    let canon = msg.encode_canonical().unwrap();

    let mut msg2 = DynamicMessage::decode_descriptor(desc, &[], &DecoderLimits::default()).unwrap();
    msg2.set_varint_by_name("id", 1).unwrap();
    assert_eq!(canon, msg2.encode_canonical().unwrap());
}

#[test]
fn oneof_descriptor_lookup() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_oneof(tpt20_core::OneofDescriptor::new("contact", vec![10, 11]));
    assert_eq!(desc.oneof_members("contact"), Some(&[10, 11][..]));
}

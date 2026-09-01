use tpt20_core::{Field, FieldDescriptor, FieldKind, MessageDescriptor, RawMessage, Value, WireClass};
use tpt20_core::ScalarKind;

#[test]
fn text_format_contains_varint() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    let mut msg = tpt20_core::DynamicMessage::with_descriptor(desc);
    msg.set_varint_by_name("id", 42).unwrap();
    let text = msg.to_text();
    assert!(text.contains("id: 42"));
}

#[test]
fn text_format_contains_string() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));
    let mut msg = tpt20_core::DynamicMessage::with_descriptor(desc);
    msg.set_string_by_name("name", "Ada").unwrap();
    let text = msg.to_text();
    assert!(text.contains("name: \"Ada\""));
}

#[test]
fn text_format_contains_bytes_as_base64() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "data", WireClass::Len, FieldKind::Scalar(ScalarKind::Bytes)));
    let mut msg = tpt20_core::DynamicMessage::with_descriptor(desc);
    msg.set_bytes_by_name("data", b"\x00\x01\x02").unwrap();
    let text = msg.to_text();
    assert!(text.contains("data: [base64 "));
}

#[test]
fn text_format_contains_fixed32() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "flag", WireClass::Fixed32, FieldKind::Scalar(ScalarKind::Fixed32)));
    let mut msg = tpt20_core::DynamicMessage::with_descriptor(desc);
    msg.set_fixed32(1, 0x12345678);
    let text = msg.to_text();
    assert!(text.contains("flag: 305441876"));
}

#[test]
fn text_format_contains_fixed64() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "val", WireClass::Fixed64, FieldKind::Scalar(ScalarKind::Fixed64)));
    let mut msg = tpt20_core::DynamicMessage::with_descriptor(desc);
    msg.set_fixed64(1, 0xABCD1234567890);
    let text = msg.to_text();
    assert!(text.contains("val: "));
}

#[test]
fn text_format_uses_numeric_id_when_no_descriptor() {
    let mut msg = tpt20_core::DynamicMessage::new();
    msg.set_varint(1, 42);
    let text = msg.to_text();
    assert!(text.contains("1: 42"));
}

#[test]
fn text_format_multiple_fields() {
    let mut desc = MessageDescriptor::new();
    desc.add_field(FieldDescriptor::new(1, "id", WireClass::Varint, FieldKind::Scalar(ScalarKind::Int64)));
    desc.add_field(FieldDescriptor::new(2, "name", WireClass::Len, FieldKind::Scalar(ScalarKind::String)));
    let mut msg = tpt20_core::DynamicMessage::with_descriptor(desc);
    msg.set_varint_by_name("id", 1).unwrap();
    msg.set_string_by_name("name", "x").unwrap();
    let text = msg.to_text();
    assert!(text.contains("id: 1"));
    assert!(text.contains("name: \"x\""));
}

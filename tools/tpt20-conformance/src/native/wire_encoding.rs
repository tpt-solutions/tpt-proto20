use tpt20_core::{Field, RawMessage, Value, WireClass};

#[test]
fn encode_decode_roundtrip_varint() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &tpt20_core::DecoderLimits::default(), tpt20_core::UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn encode_decode_roundtrip_fixed32() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Fixed32, Value::Fixed32(0x12345678)));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &tpt20_core::DecoderLimits::default(), tpt20_core::UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn encode_decode_roundtrip_fixed64() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Fixed64, Value::Fixed64(0xABCD1234567890)));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &tpt20_core::DecoderLimits::default(), tpt20_core::UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn encode_decode_roundtrip_len() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Len, Value::Len(b"hello world".to_vec())));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &tpt20_core::DecoderLimits::default(), tpt20_core::UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn encode_multiple_fields_preserves_order() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(3, WireClass::Varint, Value::Varint(3)));
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    msg.push(Field::new(2, WireClass::Len, Value::Len(b"two".to_vec())));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &tpt20_core::DecoderLimits::default(), tpt20_core::UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(back.fields.len(), 3);
    assert_eq!(back.fields[0].field_id, 3);
    assert_eq!(back.fields[1].field_id, 1);
    assert_eq!(back.fields[2].field_id, 2);
}

#[test]
fn encode_empty_message() {
    let msg = RawMessage::new();
    let bytes = msg.encode().unwrap();
    assert!(bytes.is_empty());
    let back = RawMessage::decode(&bytes, &tpt20_core::DecoderLimits::default(), tpt20_core::UnknownFieldPolicy::Preserve).unwrap();
    assert!(back.fields.is_empty());
}

#[test]
fn encode_nested_message_as_len() {
    let mut inner = RawMessage::new();
    inner.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    let inner_bytes = inner.encode().unwrap();

    let mut outer = RawMessage::new();
    outer.push(Field::new(1, WireClass::Len, Value::Len(inner_bytes)));
    let bytes = outer.encode().unwrap();
    let back = RawMessage::decode(&bytes, &tpt20_core::DecoderLimits::default(), tpt20_core::UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(back.fields.len(), 1);
    assert_eq!(back.fields[0].field_id, 1);
    match &back.fields[0].value {
        Value::Len(b) => assert_eq!(b, &inner.encode().unwrap()),
        _ => panic!("expected Len"),
    }
}

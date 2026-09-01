use tpt20_core::{DecoderLimits, Field, RawMessage, UnknownFieldPolicy, Value, WireClass};

#[test]
fn decode_varint_field() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(150)));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(back.fields[0].field_id, 1);
    assert_eq!(back.fields[0].value, Value::Varint(150));
}

#[test]
fn decode_fixed32_field() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Fixed32, Value::Fixed32(0xDEADBEEF)));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(back.fields[0].value, Value::Fixed32(0xDEADBEEF));
}

#[test]
fn decode_fixed64_field() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Fixed64, Value::Fixed64(0xABCD1234567890)));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(back.fields[0].value, Value::Fixed64(0xABCD1234567890));
}

#[test]
fn decode_len_field_utf8() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Len, Value::Len(b"hello".to_vec())));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(back.fields[0].value, Value::Len(b"hello".to_vec()));
}

#[test]
fn decode_len_field_binary() {
    let data = vec![0x00, 0x01, 0x02, 0x03];
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Len, Value::Len(data.clone())));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(back.fields[0].value, Value::Len(data));
}

#[test]
fn decode_empty_message() {
    let bytes = vec![];
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert!(back.fields.is_empty());
}

#[test]
fn decode_rejects_truncated_input() {
    let bytes = vec![0x08]; // tag for field 1 varint, no payload
    let result = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve);
    assert_eq!(result, Err(tpt20_core::DecodeError::Truncated));
}

#[test]
fn decode_rejects_truncated_fixed64() {
    let mut bytes = vec![0x09, 0x00, 0x00, 0x00]; // tag + partial 8-byte fixed64
    bytes.push(0x00);
    bytes.push(0x00);
    bytes.push(0x00);
    let result = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve);
    assert_eq!(result, Err(tpt20_core::DecodeError::Truncated));
}

#[test]
fn decode_preserves_unknown_fields() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    msg.push(Field::new(99, WireClass::Varint, Value::Varint(2)));
    let bytes = msg.encode().unwrap();
    let is_known = |id: u32| id == 1;
    let back = RawMessage::decode_filtered(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve, &is_known).unwrap();
    assert_eq!(back.fields.len(), 2);
    assert_eq!(back.fields[1].field_id, 99);
}

#[test]
fn decode_discards_unknown_fields() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    msg.push(Field::new(99, WireClass::Varint, Value::Varint(2)));
    let bytes = msg.encode().unwrap();
    let is_known = |id: u32| id == 1;
    let back = RawMessage::decode_filtered(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Discard, &is_known).unwrap();
    assert_eq!(back.fields.len(), 1);
    assert_eq!(back.fields[0].field_id, 1);
}

#[test]
fn decode_fails_on_unknown_fields_when_policy_fail() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    msg.push(Field::new(99, WireClass::Varint, Value::Varint(2)));
    let bytes = msg.encode().unwrap();
    let is_known = |id: u32| id == 1;
    let result = RawMessage::decode_filtered(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Fail, &is_known);
    assert_eq!(result, Err(tpt20_core::DecodeError::UnknownFieldForbidden));
}

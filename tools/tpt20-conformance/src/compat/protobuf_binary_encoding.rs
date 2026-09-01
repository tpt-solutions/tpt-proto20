use tpt20_compat_protobuf::wire::encode_protobuf;
use tpt20_compat_protobuf::wire::decode_protobuf;
use tpt20_core::{Field, RawMessage, Value, WireClass};

#[test]
fn encode_protobuf_roundtrips() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    msg.push(Field::new(2, WireClass::Len, Value::Len(b"hello".to_vec())));
    let bytes = encode_protobuf(&msg).unwrap();
    let back = decode_protobuf(&bytes).unwrap();
    assert_eq!(msg.fields, back.fields);
}

#[test]
fn encode_protobuf_varint() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(150)));
    let bytes = encode_protobuf(&msg).unwrap();
    let back = decode_protobuf(&bytes).unwrap();
    assert_eq!(back.fields[0].value, Value::Varint(150));
}

#[test]
fn encode_protobuf_fixed64() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Fixed64, Value::Fixed64(0xABCD1234567890)));
    let bytes = encode_protobuf(&msg).unwrap();
    let back = decode_protobuf(&bytes).unwrap();
    assert_eq!(back.fields[0].value, Value::Fixed64(0xABCD1234567890));
}

#[test]
fn encode_protobuf_fixed32() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Fixed32, Value::Fixed32(0xDEADBEEF)));
    let bytes = encode_protobuf(&msg).unwrap();
    let back = decode_protobuf(&bytes).unwrap();
    assert_eq!(back.fields[0].value, Value::Fixed32(0xDEADBEEF));
}

#[test]
fn encode_protobuf_length_delimited() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Len, Value::Len(b"hello".to_vec())));
    let bytes = encode_protobuf(&msg).unwrap();
    let back = decode_protobuf(&bytes).unwrap();
    assert_eq!(back.fields[0].value, Value::Len(b"hello".to_vec()));
}

#[test]
fn encode_protobuf_empty_message() {
    let msg = RawMessage::new();
    let bytes = encode_protobuf(&msg).unwrap();
    assert!(bytes.is_empty());
    let back = decode_protobuf(&bytes).unwrap();
    assert!(back.fields.is_empty());
}

//! Roundtrip integration tests.

use tpt20_core::{DecoderLimits, Field, RawMessage, UnknownFieldPolicy, Value, WireClass};

#[test]
fn roundtrip_simple() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    msg.push(Field::new(2, WireClass::Len, Value::Len(b"hello".to_vec())));
    msg.push(Field::new(3, WireClass::Fixed64, Value::Fixed64(7)));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn roundtrip_all_wire_classes() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    msg.push(Field::new(2, WireClass::Fixed64, Value::Fixed64(0xABCD)));
    msg.push(Field::new(3, WireClass::Len, Value::Len(b"data".to_vec())));
    let bytes = msg.encode().unwrap();
    let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn canonical_encoding_is_deterministic() {
    let mut a = RawMessage::new();
    a.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
    a.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    let mut b = RawMessage::new();
    b.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    b.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
    assert_eq!(a.encode_canonical().unwrap(), b.encode_canonical().unwrap());
}

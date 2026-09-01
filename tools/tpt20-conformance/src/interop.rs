use tpt20_core::{Field, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_compat_protobuf::wire::{decode_protobuf, encode_protobuf};

#[test]
fn native_roundtrip_protobuf_encoded() {
    let mut native = RawMessage::new();
    native.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    native.push(Field::new(2, WireClass::Len, Value::Len(b"hello".to_vec())));
    let native_bytes = native.encode().unwrap();

    let proto_bytes = encode_protobuf(&native).unwrap();
    let decoded = decode_protobuf(&proto_bytes).unwrap();
    assert_eq!(native.fields, decoded.fields);
}

#[test]
fn protobuf_decoded_matches_native_structure() {
    let proto_bytes = [0x08, 0x96, 0x01]; // field 1, varint 150
    let decoded = decode_protobuf(&proto_bytes).unwrap();
    assert_eq!(decoded.fields.len(), 1);
    assert_eq!(decoded.fields[0].field_id, 1);
    assert_eq!(decoded.fields[0].value, Value::Varint(150));

    let native = RawMessage::decode(&proto_bytes, &tpt20_core::DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(decoded.fields, native.fields);
}

#[test]
fn native_canonical_matches_protobuf_content() {
    let mut native = RawMessage::new();
    native.push(Field::new(1, WireClass::Varint, Value::Varint(7)));
    let native_bytes = native.encode().unwrap();
    let native_back = RawMessage::decode(&native_bytes, &tpt20_core::DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    let proto_bytes = encode_protobuf(&native).unwrap();
    let proto_back = decode_protobuf(&proto_bytes).unwrap();

    assert_eq!(native_back.fields, proto_back.fields);
}

#[test]
fn interop_with_length_delimited() {
    let mut native = RawMessage::new();
    native.push(Field::new(1, WireClass::Len, Value::Len(b"world".to_vec())));
    let native_bytes = native.encode().unwrap();

    let proto_bytes = encode_protobuf(&native).unwrap();
    let decoded = decode_protobuf(&proto_bytes).unwrap();

    let native_back = RawMessage::decode(&native_bytes, &tpt20_core::DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(decoded.fields, native_back.fields);
}

use tpt20_compat_protobuf::wire::decode_protobuf;
use tpt20_core::{Field, RawMessage, UnknownFieldPolicy, Value, WireClass};

#[test]
fn decode_protobuf_varint() {
    let bytes = [0x08, 0x96, 0x01]; // field 1, varint 150
    let msg = decode_protobuf(&bytes).unwrap();
    assert_eq!(msg.fields.len(), 1);
    assert_eq!(msg.fields[0].field_id, 1);
    assert_eq!(msg.fields[0].wire_class, WireClass::Varint);
    assert_eq!(msg.fields[0].value, Value::Varint(150));
}

#[test]
fn decode_protobuf_fixed64() {
    let value: u64 = 0xABCD1234567890;
    let mut bytes = vec![0x09, 0x00]; // field 1, fixed64 tag
    bytes.extend_from_slice(&value.to_le_bytes());
    let msg = decode_protobuf(&bytes).unwrap();
    assert_eq!(msg.fields[0].wire_class, WireClass::Fixed64);
    assert_eq!(msg.fields[0].value, Value::Fixed64(value));
}

#[test]
fn decode_protobuf_fixed32() {
    let value: u32 = 0xDEADBEEF;
    let mut bytes = vec![0x0D, 0x00]; // field 1, fixed32 tag
    bytes.extend_from_slice(&value.to_le_bytes());
    let msg = decode_protobuf(&bytes).unwrap();
    assert_eq!(msg.fields[0].wire_class, WireClass::Fixed32);
    assert_eq!(msg.fields[0].value, Value::Fixed32(value));
}

#[test]
fn decode_protobuf_length_delimited() {
    let payload = b"hello";
    let mut bytes = vec![0x0A]; // field 1, length-delimited tag
    bytes.push(payload.len() as u8);
    bytes.extend_from_slice(payload);
    let msg = decode_protobuf(&bytes).unwrap();
    assert_eq!(msg.fields[0].wire_class, WireClass::Len);
    assert_eq!(msg.fields[0].value, Value::Len(b"hello".to_vec()));
}

#[test]
fn decode_protobuf_multiple_fields() {
    let mut bytes = vec![0x08, 0x01]; // field 1, varint 1
    bytes.push(0x12); // field 2, length-delimited
    bytes.push(0x05);
    bytes.extend_from_slice(b"world");
    let msg = decode_protobuf(&bytes).unwrap();
    assert_eq!(msg.fields.len(), 2);
    assert_eq!(msg.fields[0].field_id, 1);
    assert_eq!(msg.fields[1].field_id, 2);
}

#[test]
fn decode_protobuf_empty() {
    let msg = decode_protobuf(&[]).unwrap();
    assert!(msg.fields.is_empty());
}

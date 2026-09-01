//! Integration tests for `tpt20-compat-protobuf`.
//!
//! Covers:
//! - Proto schema import (proto2/proto3)
//! - Protobuf wire adapter round-trip
//! - Golden-vector / differential tests

use tpt20_compat_protobuf::{lex_proto, parse_proto, lower, wire};

// ===========================================================================
// Golden-vector tests (differential against reference protobuf encoding)
// ===========================================================================

#[test]
fn golden_varint_field1_150() {
    let bytes = [0x08, 0x96, 0x01];
    let msg = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg.fields.len(), 1);
    assert_eq!(msg.fields[0].field_id, 1);
    assert_eq!(msg.fields[0].wire_class, tpt20_core::wire::WireClass::Varint);
    assert_eq!(msg.fields[0].value, tpt20_core::message::Value::Varint(150));
    let re = wire::encode_protobuf(&msg).expect("encode");
    assert_eq!(&re[..], &bytes[..]);
}

#[test]
fn golden_fixed32_field5() {
    let bytes = [0x2d, 0x04, 0x03, 0x02, 0x01];
    let msg = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg.fields[0].field_id, 5);
    assert_eq!(msg.fields[0].wire_class, tpt20_core::wire::WireClass::Fixed32);
    assert_eq!(msg.fields[0].value, tpt20_core::message::Value::Fixed32(0x01020304));
    let re = wire::encode_protobuf(&msg).expect("encode");
    assert_eq!(&re[..], &bytes[..]);
}

#[test]
fn golden_fixed64_field1() {
    let bytes = [0x09, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11];
    let msg = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg.fields[0].field_id, 1);
    assert_eq!(msg.fields[0].wire_class, tpt20_core::wire::WireClass::Fixed64);
    assert_eq!(msg.fields[0].value, tpt20_core::message::Value::Fixed64(0x1122334455667788));
    let re = wire::encode_protobuf(&msg).expect("encode");
    assert_eq!(&re[..], &bytes[..]);
}

#[test]
fn golden_len_field2() {
    let bytes = [0x12, 0x07, b't', b'e', b's', b't', b'i', b'n', b'g'];
    let msg = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg.fields[0].field_id, 2);
    assert_eq!(msg.fields[0].wire_class, tpt20_core::wire::WireClass::Len);
    assert_eq!(msg.fields[0].value, tpt20_core::message::Value::Len(b"testing".to_vec()));
    let re = wire::encode_protobuf(&msg).expect("encode");
    assert_eq!(&re[..], &bytes[..]);
}

#[test]
fn golden_zigzag_negative() {
    let bytes = [0x08, 0x01];
    let msg = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg.fields[0].value, tpt20_core::message::Value::Varint(1));
    let re = wire::encode_protobuf(&msg).expect("encode");
    assert_eq!(&re[..], &bytes[..]);
}

#[test]
fn golden_multiple_fields() {
    let bytes = [
        0x08, 0x96, 0x01, // field 1, varint 150
        0x12, 0x03, b'f', b'o', b'o', // field 2, len "foo"
        0x2d, 0xef, 0xbe, 0xad, 0xde, // field 5, fixed32
    ];
    let msg = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg.fields.len(), 3);
    assert_eq!(msg.fields[0].field_id, 1);
    assert_eq!(msg.fields[1].field_id, 2);
    assert_eq!(msg.fields[2].field_id, 5);
    let re = wire::encode_protobuf(&msg).expect("encode");
    assert_eq!(&re[..], &bytes[..]);
}

// ===========================================================================
// Wire adapter round-trip tests
// ===========================================================================

#[test]
fn roundtrip_empty_message() {
    let msg = tpt20_core::message::RawMessage::new();
    let bytes = wire::encode_protobuf(&msg).expect("encode");
    let back = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg, back);
}

#[test]
fn roundtrip_all_wire_classes() {
    let mut msg = tpt20_core::message::RawMessage::new();
    msg.push(tpt20_core::message::Field::new(
        1,
        tpt20_core::wire::WireClass::Varint,
        tpt20_core::message::Value::Varint(7),
    ));
    msg.push(tpt20_core::message::Field::new(
        5,
        tpt20_core::wire::WireClass::Fixed32,
        tpt20_core::message::Value::Fixed32(0xdeadbeef),
    ));
    msg.push(tpt20_core::message::Field::new(
        6,
        tpt20_core::wire::WireClass::Fixed64,
        tpt20_core::message::Value::Fixed64(0x8899aabbccddeeff),
    ));
    msg.push(tpt20_core::message::Field::new(
        7,
        tpt20_core::wire::WireClass::Len,
        tpt20_core::message::Value::Len(b"payload".to_vec()),
    ));
    let bytes = wire::encode_protobuf(&msg).expect("encode");
    let back = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg, back);
}

#[test]
fn roundtrip_large_varint() {
    let mut msg = tpt20_core::message::RawMessage::new();
    msg.push(tpt20_core::message::Field::new(
        1,
        tpt20_core::wire::WireClass::Varint,
        tpt20_core::message::Value::Varint(u64::MAX),
    ));
    let bytes = wire::encode_protobuf(&msg).expect("encode");
    let back = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg, back);
}

#[test]
fn roundtrip_field_id_zero() {
    let mut msg = tpt20_core::message::RawMessage::new();
    msg.push(tpt20_core::message::Field::new(
        0,
        tpt20_core::wire::WireClass::Varint,
        tpt20_core::message::Value::Varint(1),
    ));
    let bytes = wire::encode_protobuf(&msg).expect("encode");
    let back = wire::decode_protobuf(&bytes).expect("decode");
    assert_eq!(msg, back);
}

// ===========================================================================
// Proto schema import tests
// ===========================================================================

#[test]
fn import_proto2_simple_message() {
    let src = r#"syntax = "proto2";
package user.v1;

message User {
  required int32 id = 1;
  optional string name = 2;
  repeated string tags = 3;
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.package, Some("user.v1".into()));
    assert_eq!(ir.messages.len(), 1);
    let msg = &ir.messages[0];
    assert_eq!(msg.name, "User");
    assert_eq!(msg.fields.len(), 3);

    // id
    assert_eq!(msg.fields[0].id, 1);
    assert_eq!(msg.fields[0].name, "id");
    assert!(matches!(msg.fields[0].presence, tpt20_ir::Presence::Explicit));

    // name
    assert_eq!(msg.fields[1].id, 2);
    assert_eq!(msg.fields[1].name, "name");
    assert!(matches!(msg.fields[1].presence, tpt20_ir::Presence::Explicit));

    // tags
    assert_eq!(msg.fields[2].id, 3);
    assert_eq!(msg.fields[2].name, "tags");
}

#[test]
fn import_proto3_message() {
    let src = r#"syntax = "proto3";
package example.v1;

message SearchRequest {
  string query = 1;
  int32 page_number = 2;
  int32 result_per_page = 3;
  bool deprecated_field = 4 [deprecated = true];
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.package, Some("example.v1".into()));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].fields.len(), 4);
}

#[test]
fn import_proto2_enum() {
    let src = r#"syntax = "proto2";
package enums.v1;

enum Status {
  UNKNOWN = 0;
  STARTED = 1;
  STOPPED = 2;
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.enums.len(), 1);
    let en = &ir.enums[0];
    assert_eq!(en.name, "Status");
    assert_eq!(en.values.len(), 3);
    assert_eq!(en.values[0].number, 0);
    assert_eq!(en.values[1].number, 1);
    assert_eq!(en.values[2].number, 2);
}

#[test]
fn import_proto2_oneof() {
    let src = r#"syntax = "proto2";
package oneofs.v1;

message SampleMessage {
  oneof test_oneof {
    int32 foo = 4;
    string bar = 9;
  }
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].oneofs.len(), 1);
    let oneof = &ir.messages[0].oneofs[0];
    assert_eq!(oneof.name, "test_oneof");
    assert_eq!(oneof.fields.len(), 2);
    assert_eq!(oneof.fields[0].name, "foo");
    assert_eq!(oneof.fields[1].name, "bar");
}

#[test]
fn import_proto2_map_field() {
    let src = r#"syntax = "proto2";
package maps.v1;

message MyMessage {
  map<string, int32> my_map = 1;
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].fields.len(), 1);
    // map fields are lowered as FieldLabel::Singular with a map type ref
    match &ir.messages[0].fields[0].label {
        tpt20_ir::FieldLabelIr::Singular(t) => {
            assert!(t.path[0].starts_with("map<"));
        }
        other => panic!("expected singular map type, got {:?}", other),
    }
}

#[test]
fn import_proto2_service() {
    let src = r#"syntax = "proto2";
package rpc.v1;

service SearchService {
  rpc Search (SearchRequest) returns (SearchResponse);
  rpc StreamSearch (SearchRequest) returns (stream SearchResponse);
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.services.len(), 1);
    assert_eq!(ir.services[0].name, "SearchService");
    assert_eq!(ir.services[0].methods.len(), 2);
    assert_eq!(ir.services[0].methods[0].name, "Search");
    assert_eq!(ir.services[0].methods[1].name, "StreamSearch");
    assert!(!ir.services[0].methods[0].response_streaming);
    assert!(ir.services[0].methods[1].response_streaming);
}

#[test]
fn import_proto2_reserved() {
    let src = r#"syntax = "proto2";
package reserved.v1;

message MyMessage {
  reserved 2, 15, 9 to 11;
  reserved "foo", "bar";
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.messages.len(), 1);
    let msg = &ir.messages[0];
    assert_eq!(msg.reserved.len(), 1);
    let r = &msg.reserved[0];
    assert_eq!(r.ids.len(), 3);
    assert_eq!(r.names.len(), 2);
}

#[test]
fn import_proto2_extend() {
    let src = r#"syntax = "proto2";
package extend.v1;

message Base {
  int32 base_field = 1;
}

extend Base {
  optional string ext_field = 100;
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.extensions.len(), 1);
    assert_eq!(ir.extensions[0].message_type, vec!["Base"]);
    assert_eq!(ir.extensions[0].fields.len(), 1);
    assert_eq!(ir.extensions[0].fields[0].number, 100);
}

#[test]
fn import_proto2_nested_message() {
    let src = r#"syntax = "proto2";
package nested.v1;

message Outer {
  message Inner {
    int32 value = 1;
  }
  Inner inner = 1;
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].messages.len(), 1);
    assert_eq!(ir.messages[0].messages[0].name, "Inner");
}

#[test]
fn import_proto2_options_preserved() {
    let src = r#"syntax = "proto2";
package opts.v1;

option java_package = "com.example";

message Foo {
  option (my_option) = "hello";
  int32 x = 1;
}
"#;
    let tokens = lex_proto(src).expect("lex");
    let proto = parse_proto(tokens).expect("parse");
    let ir = lower(proto).expect("lower");

    assert_eq!(ir.options.len(), 1);
    assert_eq!(ir.options[0].name, "java_package");
    assert_eq!(ir.messages[0].options.len(), 1);
}

// ===========================================================================
// Differential tests: proto2 vs proto3 wire compatibility for scalar repeated
// ===========================================================================

#[test]
fn differential_repeated_packed_vs_unpacked_accepts_both() {
    // tpt20-core's RawMessage decode treats all fields as known by default,
    // preserving both packed and unpacked forms. This verifies the adapter
    // layer simply passes through whatever protobuf bytes are given.
    let packed = [
        0x0a, // field 1, wire type 2 (len-delimited)
        0x02, // length 2
        0x03, 0x03, // packed varints 3, 3
    ];
    let unpacked = [
        0x08, 0x03, // field 1, varint 3
        0x08, 0x03, // field 1, varint 3
    ];
    let p = wire::decode_protobuf(&packed).expect("packed");
    let u = wire::decode_protobuf(&unpacked).expect("unpacked");
    // Both decode successfully and contain the same field IDs and wire classes.
    assert_eq!(p.fields.len(), 1);
    assert_eq!(u.fields.len(), 2);
    assert_eq!(p.fields[0].field_id, u.fields[0].field_id);
    assert_eq!(p.fields[0].wire_class, u.fields[0].wire_class);
}

#[test]
fn differential_wire_type_translation_fixed32_uses_protobuf_wire_5() {
    // Verify that encoding a Fixed32 field produces protobuf wire type 5.
    let mut msg = tpt20_core::message::RawMessage::new();
    msg.push(tpt20_core::message::Field::new(
        1,
        tpt20_core::wire::WireClass::Fixed32,
        tpt20_core::message::Value::Fixed32(0x0a0b0c0d),
    ));
    let bytes = wire::encode_protobuf(&msg).expect("encode");
    // First byte is tag = (1 << 3) | 5 = 13 = 0x0d
    assert_eq!(bytes[0], 0x0d);
    // Rest is little-endian payload
    assert_eq!(&bytes[1..], &[0x0d, 0x0c, 0x0b, 0x0a]);
}

#[test]
fn differential_wire_type_translation_fixed64_uses_protobuf_wire_1() {
    let mut msg = tpt20_core::message::RawMessage::new();
    msg.push(tpt20_core::message::Field::new(
        1,
        tpt20_core::wire::WireClass::Fixed64,
        tpt20_core::message::Value::Fixed64(0x1122334455667788),
    ));
    let bytes = wire::encode_protobuf(&msg).expect("encode");
    // First byte is tag = (1 << 3) | 1 = 9 = 0x09
    assert_eq!(bytes[0], 0x09);
}

#[test]
fn differential_len_uses_protobuf_wire_2() {
    let mut msg = tpt20_core::message::RawMessage::new();
    msg.push(tpt20_core::message::Field::new(
        1,
        tpt20_core::wire::WireClass::Len,
        tpt20_core::message::Value::Len(b"hi".to_vec()),
    ));
    let bytes = wire::encode_protobuf(&msg).expect("encode");
    assert_eq!(bytes[0], 0x0a); // (1 << 3) | 2 = 10
}

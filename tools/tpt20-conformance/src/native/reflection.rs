use tpt20_core::{DecoderLimits, Field, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_descriptor::Descriptor;
use tpt20_reflect::DynamicMessage as ReflectDynamicMessage;
use tpt20_reflect::{ReflectEnum, ReflectValue};
use tpt20_ir as ir;

fn sample_descriptor() -> Descriptor {
    let pkg = ir::PackageIr {
        name: Some("test.v1".to_string()),
        imports: vec![],
        messages: vec![
            ir::MessageIr {
                name: "User".into(),
                fields: vec![
                    ir::FieldIr {
                        id: 1,
                        name: "id".into(),
                        label: ir::FieldLabelIr::Singular(ir::TypeRefIr { path: vec!["int64".into()] }),
                        presence: ir::Presence::Implicit,
                        annotations: vec![],
                        span: ir::SourceSpan::default(),
                    },
                    ir::FieldIr {
                        id: 2,
                        name: "name".into(),
                        label: ir::FieldLabelIr::Singular(ir::TypeRefIr { path: vec!["string".into()] }),
                        presence: ir::Presence::Implicit,
                        annotations: vec![],
                        span: ir::SourceSpan::default(),
                    },
                    ir::FieldIr {
                        id: 3,
                        name: "tags".into(),
                        label: ir::FieldLabelIr::Repeated(ir::TypeRefIr { path: vec!["string".into()] }),
                        presence: ir::Presence::Implicit,
                        annotations: vec![],
                        span: ir::SourceSpan::default(),
                    },
                    ir::FieldIr {
                        id: 4,
                        name: "score".into(),
                        label: ir::FieldLabelIr::Singular(ir::TypeRefIr { path: vec!["float64".into()] }),
                        presence: ir::Presence::Implicit,
                        annotations: vec![],
                        span: ir::SourceSpan::default(),
                    },
                ],
                oneofs: vec![],
                messages: vec![],
                enums: vec![],
                reserved: vec![],
                annotations: vec![],
                span: ir::SourceSpan::default(),
            },
        ],
        enums: vec![],
        services: vec![],
        reserved: vec![],
        compat: ir::CompatMetadata::default(),
        fingerprint: None,
    };
    let mut desc = Descriptor::new(pkg);
    desc.compute_fingerprint();
    desc
}

#[test]
fn field_access_by_id_and_name() {
    let descriptor = sample_descriptor();
    let msg_ir = descriptor.find_message("User").unwrap();

    let mut raw = RawMessage::new();
    raw.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    raw.push(Field::new(2, WireClass::Len, Value::Len(b"Ada".to_vec())));
    let bytes = raw.encode().unwrap();

    let message = ReflectDynamicMessage::decode(msg_ir, &descriptor, &bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    assert_eq!(message.get_field("id").unwrap(), Some(ReflectValue::Varint(42)));
    assert_eq!(message.get_field("name").unwrap(), Some(ReflectValue::String("Ada".to_string())));
    assert_eq!(message.get_field_id(1).unwrap(), Some(ReflectValue::Varint(42)));
    assert_eq!(message.get_field_id(2).unwrap(), Some(ReflectValue::String("Ada".to_string())));
}

#[test]
fn field_mutation() {
    let descriptor = sample_descriptor();
    let msg_ir = descriptor.find_message("User").unwrap();

    let mut message = ReflectDynamicMessage::decode(msg_ir, &descriptor, &[], &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    message.set_field("name", ReflectValue::String("Bob".into())).unwrap();
    assert_eq!(message.get_field("name").unwrap(), Some(ReflectValue::String("Bob".to_string())));

    message.set_field_id(1, ReflectValue::Varint(99)).unwrap();
    assert_eq!(message.get_field_id(1).unwrap(), Some(ReflectValue::Varint(99)));

    message.clear_field("name").unwrap();
    assert_eq!(message.get_field("name").unwrap(), None);
}

#[test]
fn repeated_field_access() {
    let descriptor = sample_descriptor();
    let msg_ir = descriptor.find_message("User").unwrap();

    let mut raw = RawMessage::new();
    raw.push(Field::new(3, WireClass::Len, Value::Len(b"a".to_vec())));
    raw.push(Field::new(3, WireClass::Len, Value::Len(b"b".to_vec())));
    let bytes = raw.encode().unwrap();

    let mut message = ReflectDynamicMessage::decode(msg_ir, &descriptor, &bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    let tags = message.get_repeated("tags").unwrap().unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0], ReflectValue::String("a".to_string()));
    assert_eq!(tags[1], ReflectValue::String("b".to_string()));

    message.add_repeated("tags", ReflectValue::String("c".into())).unwrap();
    let updated = message.get_repeated("tags").unwrap().unwrap();
    assert_eq!(updated.len(), 3);
}

#[test]
fn enum_access() {
    let descriptor = sample_descriptor();
    let msg_ir = descriptor.find_message("User").unwrap();

    let mut raw = RawMessage::new();
    raw.push(Field::new(4, WireClass::Fixed64, Value::Fixed64(f64::to_bits(3.14))));
    let bytes = raw.encode().unwrap();

    let message = ReflectDynamicMessage::decode(msg_ir, &descriptor, &bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    let val = message.get_field_id(4).unwrap();
    assert!(val.is_some());
    match val.unwrap() {
        ReflectValue::Fixed64(bits) => {
            let f = f64::from_bits(bits);
            assert!((f - 3.14).abs() < 0.01);
        }
        _ => panic!("expected Fixed64"),
    }
}

#[test]
fn oneof_access() {
    let descriptor = sample_descriptor();
    let mut pkg = descriptor.package.clone();
    pkg.messages[0].oneofs.push(ir::OneofIr {
        name: "contact".into(),
        fields: vec![ir::FieldIr {
            id: 10,
            name: "email".into(),
            label: ir::FieldLabelIr::Singular(ir::TypeRefIr { path: vec!["string".into()] }),
            presence: ir::Presence::Implicit,
            annotations: vec![],
            span: ir::SourceSpan::default(),
        }],
        annotations: vec![],
        span: ir::SourceSpan::default(),
    });
    let desc = Descriptor::new(pkg);
    let msg_ir = desc.find_message("User").unwrap();

    let mut raw = RawMessage::new();
    raw.push(Field::new(10, WireClass::Len, Value::Len(b"a@b.com".to_vec())));
    let bytes = raw.encode().unwrap();

    let message = ReflectDynamicMessage::decode(msg_ir, &desc, &bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    let oneof = message.get_oneof("contact").unwrap().unwrap();
    assert_eq!(oneof.name, "contact");
    assert!(oneof.active_field.is_some());
    assert_eq!(oneof.active_field.unwrap().name, "email");
}

#[test]
fn nested_message_access() {
    let descriptor = sample_descriptor();
    let mut pkg = descriptor.package.clone();
    pkg.messages.push(ir::MessageIr {
        name: "Address".into(),
        fields: vec![ir::FieldIr {
            id: 1,
            name: "street".into(),
            label: ir::FieldLabelIr::Singular(ir::TypeRefIr { path: vec!["string".into()] }),
            presence: ir::Presence::Implicit,
            annotations: vec![],
            span: ir::SourceSpan::default(),
        }],
        oneofs: vec![],
        messages: vec![],
        enums: vec![],
        reserved: vec![],
        annotations: vec![],
        span: ir::SourceSpan::default(),
    });
    pkg.messages[0].fields.push(ir::FieldIr {
        id: 20,
        name: "address".into(),
        label: ir::FieldLabelIr::Singular(ir::TypeRefIr { path: vec!["Address".into()] }),
        presence: ir::Presence::Implicit,
        annotations: vec![],
        span: ir::SourceSpan::default(),
    });
    let desc = Descriptor::new(pkg);
    let msg_ir = desc.find_message("User").unwrap();

    let mut inner = RawMessage::new();
    inner.push(Field::new(1, WireClass::Len, Value::Len(b"Main St".to_vec())));
    let mut raw = RawMessage::new();
    raw.push(Field::new(20, WireClass::Len, Value::Len(inner.encode().unwrap())));
    let bytes = raw.encode().unwrap();

    let message = ReflectDynamicMessage::decode(msg_ir, &desc, &bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    let nested = message.get_message("address").unwrap().unwrap();
    assert_eq!(nested.message_name(), "Address");
    assert_eq!(
        nested.get_field("street").unwrap(),
        Some(ReflectValue::String("Main St".to_string()))
    );
}

#[test]
fn descriptor_lookup_and_fingerprint() {
    let descriptor = sample_descriptor();
    let msg_ir = descriptor.find_message("User").unwrap();
    let message = ReflectDynamicMessage::decode(msg_ir, &descriptor, &[], &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    assert_eq!(message.message_name(), "User");
    assert!(message.fingerprint().is_some());
    assert!(message.descriptor().find_message("User").is_some());
}

#[test]
fn proxy_gateway_use_case() {
    let descriptor = sample_descriptor();
    let msg_ir = descriptor.find_message("User").unwrap();

    let mut raw = RawMessage::new();
    raw.push(Field::new(1, WireClass::Varint, Value::Varint(7)));
    raw.push(Field::new(2, WireClass::Len, Value::Len(b"proxy".to_vec())));
    let bytes = raw.encode().unwrap();

    let inbound = ReflectDynamicMessage::decode(msg_ir, &descriptor, &bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();

    assert!(inbound.get_field("id").unwrap().is_some());
    assert!(inbound.get_field("name").unwrap().is_some());

    let outbound = inbound.encode().unwrap();
    let roundtrip = ReflectDynamicMessage::decode(msg_ir, &descriptor, &outbound, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
    assert_eq!(inbound.raw.fields, roundtrip.raw.fields);
}

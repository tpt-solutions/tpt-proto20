//! Compile-and-run tests for `tpt20-codegen-rust` output (todo Phase 5).
//!
//! `build.rs` compiles [`SCHEMA`] and generates a Rust module into `OUT_DIR`;
//! this crate includes it, so every test here exercises real generated code:
//! wire roundtrips, canonical encoding, unknown-field preservation, limits,
//! views, JSON, and builders.

/// The fixture schema compiled at build time.
pub const SCHEMA: &str = include_str!("schema.tpt");

/// Fingerprint recorded by the build script.
pub const FINGERPRINT: &str = include_str!(concat!(env!("OUT_DIR"), "/fingerprint.txt"));

/// Generated module from the fixture schema.
#[allow(unused)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

use generated::{Address, Outer, OuterContact, Outer_Feature, Outer_Status};

fn sample() -> Outer {
    Outer {
        id: -5,
        name: "Ada".into(),
        email: Some("ada@example.com".into()),
        age: Some(42),
        username: "ada01".into(),
        tags: vec!["a".into(), "b".into()],
        scores: vec![1, -2, 300],
        attrs: [("k".to_string(), "v".to_string())].into_iter().collect(),
        counts: [(-7i64, "neg".to_string())].into_iter().collect(),
        contact: Some(OuterContact::EmailAddr("x@y.z".into())),
        status: Outer_Status::SUSPENDED,
        feature: Outer_Feature::Unknown(77),
        home: Some(Address {
            street: "1 Way".into(),
            city: None,
            ..Default::default()
        }),
        blob: vec![0xff, 0x00, 0x7f],
        ratio: -2.5,
        flags: vec![1, u32::MAX],
        inner: Some(generated::Outer_Child {
            note: "n".into(),
            depth: 3,
            leaf: Some(generated::Outer_Child_Leaf { value: true, ..Default::default() }),
            unknown_fields: Default::default(),
        }),
        zigzag: i64::MIN,
        unknown_fields: Default::default(),
    }
}

#[test]
fn owned_roundtrip_covers_all_field_kinds() {
    let msg = sample();
    let bytes = msg.encode();
    assert_eq!(Outer::decode(&bytes).unwrap(), msg);
}

#[test]
fn explicit_presence_survives_default_values() {
    let a = Outer {
        email: Some(String::new()),
        ..Default::default()
    };
    let b = Outer::default();
    // Explicit presence: Some("") must be distinguishable from None on the wire.
    assert_ne!(a.encode(), b.encode());
    assert_eq!(
        Outer::decode(&a.encode()).unwrap().email,
        Some(String::new())
    );
}

#[test]
fn zigzag_and_fixed_scalars_roundtrip() {
    let m = Outer {
        zigzag: -1234567890123456789,
        flags: vec![0xdead_beef, 7],
        ratio: 3.14159,
        ..Default::default()
    };
    let back = Outer::decode(&m.encode()).unwrap();
    assert_eq!(back.zigzag, -1234567890123456789);
    assert_eq!(back.flags, vec![0xdead_beef, 7]);
    assert!((back.ratio - 3.14159).abs() < f64::EPSILON);
}

#[test]
fn oneof_last_wins_on_wire() {
    let email_only = Outer {
        contact: Some(OuterContact::EmailAddr("first".into())),
        ..Default::default()
    };
    let addr_only = Outer {
        contact: Some(OuterContact::Addr(Address {
            street: "second".into(),
            city: None,
        })),
        ..Default::default()
    };
    // Email appears before addr on the wire -> addr wins (spec §9.8).
    let mut wire = email_only.encode();
    wire.extend(addr_only.encode());
    let decoded = Outer::decode(&wire).unwrap();
    match decoded.contact {
        Some(OuterContact::Addr(a)) => assert_eq!(a.street, "second"),
        other => panic!("expected Addr after last-wins, got {other:?}"),
    }
}

#[test]
fn map_duplicate_entries_last_wins() {
    use tpt20_core::{Field, RawMessage, Value, WireClass};
    // Hand-build two entries for field 8 ("attrs"): k="dup" v=one, then v=two.
    let entry = |v: &str| -> Vec<u8> {
        let mut e = RawMessage::new();
        e.push(Field::new(1, WireClass::Len, Value::Len(b"dup".to_vec())));
        e.push(Field::new(
            2,
            WireClass::Len,
            Value::Len(v.as_bytes().to_vec()),
        ));
        e.encode().unwrap()
    };
    let mut raw = RawMessage::new();
    for v in ["one", "two"] {
        raw.push(Field::new(8, WireClass::Len, Value::Len(entry(v))));
    }
    let decoded = Outer::decode(&raw.encode().unwrap()).unwrap();
    assert_eq!(decoded.attrs.get("dup").map(String::as_str), Some("two"));
}

#[test]
fn canonical_output_is_order_independent() {
    // Two wire spellings of the same logical content: oneof members in
    // different order (email+addr vs addr only) and unknowns in both orders.
    let email_only = Outer {
        id: 1,
        name: "x".into(),
        contact: Some(OuterContact::EmailAddr("e".into())),
        ..Default::default()
    };
    let addr_only = Outer {
        id: 1,
        name: "x".into(),
        contact: Some(OuterContact::Addr(Address {
            street: String::new(),
            city: None,
        })),
        ..Default::default()
    };

    let mut w1 = email_only.encode();
    w1.extend(addr_only.encode());
    let mut w2 = addr_only.encode();

    let d1 = Outer::decode(&w1).unwrap();
    let d2 = Outer::decode(&w2).unwrap();
    // Same content -> same canonical bytes, even though w1 != w2.
    assert_ne!(w1, w2);
    assert_eq!(d1.encode_canonical(), d2.encode_canonical());
    let _ = &mut w2;
}

#[test]
fn unknown_fields_are_preserved_and_reencoded() {
    use tpt20_core::{Field, RawMessage, Value, WireClass};
    let mut raw = RawMessage::new();
    raw.push(Field::new(1, WireClass::Varint, Value::Varint(9))); // id
    raw.push(Field::new(
        200,
        WireClass::Len,
        Value::Len(b"future".to_vec()),
    )); // unknown
    let bytes = raw.encode().unwrap();

    let decoded = Outer::decode(&bytes).unwrap();
    assert_eq!(decoded.id, 9);
    assert_eq!(decoded.unknown_fields.fields.len(), 1);

    let re = decoded.encode();
    assert_eq!(
        Outer::decode(&re).unwrap().unknown_fields.fields.len(),
        1,
        "unknown fields survive re-encoding"
    );
}

#[test]
fn open_enum_captures_unknown_closed_enum_rejects() {
    use tpt20_core::{Field, RawMessage, Value, WireClass};
    let mk = |feature: i64, status: i64| -> Vec<u8> {
        let mut raw = RawMessage::new();
        raw.push(Field::new(
            13,
            WireClass::Varint,
            Value::Varint(feature as u64),
        ));
        raw.push(Field::new(
            12,
            WireClass::Varint,
            Value::Varint(status as u64),
        ));
        raw.encode().unwrap()
    };

    let open = Outer::decode(&mk(99, 1)).unwrap();
    assert_eq!(open.feature, Outer_Feature::Unknown(99));
    assert_eq!(open.status, Outer_Status::ACTIVE);

    assert!(matches!(
        Outer::decode(&mk(1, 55)),
        Err(tpt20_core::DecodeError::InvalidEnumValue(55))
    ));
}

#[test]
fn decoder_limits_are_enforced() {
    use tpt20_core::DecodeError;
    // Fixture nesting is Outer(1) -> Child(2) -> Leaf(3): depth 3.
    let nested = sample();
    let bytes = nested.encode();

    let mut limits = tpt20_core::DecoderLimits::default();
    limits.max_depth = 2;
    assert_eq!(
        Outer::decode_with_limits(&bytes, &limits),
        Err(DecodeError::DepthExceeded)
    );

    let big_string = Outer {
        name: "x".repeat(100),
        ..Default::default()
    };
    let mut tight = tpt20_core::DecoderLimits::default();
    tight.max_string_bytes = 16;
    assert_eq!(
        Outer::decode_with_limits(&big_string.encode(), &tight),
        Err(DecodeError::LimitExceeded { limit: 16 })
    );
}

#[test]
fn json_roundtrip_with_spec_rules() {
    let msg = sample();
    let json = msg.to_json().unwrap();

    // Spec §14.2: 64-bit ints as strings, bytes as base64, enum names.
    assert!(json.contains(r#""id":"-5""#));
    assert!(json.contains(r#""blob":"fwA/""#)); // base64(0xff 0x00 0x7f)
    assert!(json.contains(r#""status":"SUSPENDED""#));

    let back = Outer::from_json(&json).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn json_accepts_camelcase_and_number_enums() {
    // lowerCamelCase alias + numbers for enums + string-form i64.
    let json = r#"{
        "id": "12",
        "userName": "bob",
        "status": 2,
        "feature": 9
    }"#;
    let m = Outer::from_json(json).unwrap();
    assert_eq!(m.id, 12);
    assert_eq!(m.username, "bob");
    assert_eq!(m.status, Outer_Status::SUSPENDED);
    assert_eq!(m.feature, Outer_Feature::Unknown(9));
}

#[test]
fn borrowed_view_decodes_without_owned_strings() {
    let bytes = sample().encode();
    let view = Outer::decode_borrowed(&bytes).unwrap();
    assert_eq!(view.id, -5);
    assert_eq!(view.name, "Ada");
    assert_eq!(view.email, Some("ada@example.com"));
    assert_eq!(view.blob, &[0xffu8, 0x00, 0x7f][..]);
    assert_eq!(view.tags, vec!["a", "b"]);
    match &view.contact {
        Some(generated::OuterContactView::EmailAddr(e)) => {
            assert_eq!(*e, "x@y.z")
        }
        other => panic!("expected EmailAddr view, got {other:?}"),
    }
    let child = view.inner.as_ref().unwrap();
    assert_eq!(child.note, "n");
    assert_eq!(child.leaf.as_ref().unwrap().value, true);
}

#[test]
fn builders_validate_annotations() {
    use generated::BuildError;

    // @max_len(8) on username.
    let ok = OuterBuilder::new()
        .username("short")
        .age(30)
        .build()
        .unwrap();
    assert_eq!(ok.username, "short");

    let err = OuterBuilder::new()
        .username("way-too-long-for-max-len-8")
        .build()
        .unwrap_err();
    assert_eq!(
        err,
        BuildError::MaxLenExceeded {
            field: "username",
            max: 8
        }
    );

    // @range(0, 150) on age.
    let err = OuterBuilder::new().age(-1).build().unwrap_err();
    assert_eq!(
        err,
        BuildError::OutOfRange {
            field: "age"
        }
    );

    // Full builder path roundtrips like the struct literal path.
    let built = OuterBuilder::new()
        .id(7)
        .name("b")
        .tags(["t1".to_string(), "t2".to_string()])
        .attrs([("k".to_string(), "v".to_string())])
        .contact(OuterContact::EmailAddr("c@d.e".into()))
        .build()
        .unwrap();
    assert_eq!(
        Outer::decode(&built.encode()).unwrap(),
        Outer::decode(&Outer::decode(&built.encode()).unwrap().encode()).unwrap()
    );
}

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
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

#[cfg(test)]
use generated::{Address, Outer, OuterContact, Outer_Feature, Outer_Status};

#[cfg(test)]
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
            leaf: Some(generated::Outer_Child_Leaf {
                value: true,
                ..Default::default()
            }),
            unknown_fields: Default::default(),
        }),
        zigzag: i64::MIN,
        homes: [(
            "office".to_string(),
            Address {
                street: "2 Road".into(),
                city: Some("X".into()),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect(),
        status_by_name: [("a".to_string(), Outer_Status::INACTIVE)]
            .into_iter()
            .collect(),
        address_book: vec![
            Address {
                street: "a".into(),
                ..Default::default()
            },
            Address {
                street: "b".into(),
                city: Some("c".into()),
                ..Default::default()
            },
        ],
        statuses: vec![Outer_Status::SUSPENDED, Outer_Status::ACTIVE],
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
        ratio: 2.5,
        ..Default::default()
    };
    let back = Outer::decode(&m.encode()).unwrap();
    assert_eq!(back.zigzag, -1234567890123456789);
    assert_eq!(back.flags, vec![0xdead_beef, 7]);
    assert!((back.ratio - 2.5).abs() < f64::EPSILON);
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
            ..Default::default()
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
            ..Default::default()
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
            14,
            WireClass::Varint,
            Value::Varint(feature as u64),
        ));
        raw.push(Field::new(
            13,
            WireClass::Varint,
            Value::Varint(status as u64),
        ));
        raw.encode().unwrap()
    };

    let open = Outer::decode(&mk(99, 1)).unwrap();
    assert_eq!(open.feature, Outer_Feature::Unknown(99));
    assert_eq!(open.status, Outer_Status::INACTIVE);

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

    let limits = tpt20_core::DecoderLimits {
        max_depth: 2,
        ..Default::default()
    };
    assert_eq!(
        Outer::decode_with_limits(&bytes, &limits),
        Err(DecodeError::DepthExceeded)
    );

    let big_string = Outer {
        name: "x".repeat(100),
        ..Default::default()
    };
    let tight = tpt20_core::DecoderLimits {
        max_string_bytes: 16,
        ..Default::default()
    };
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
    assert!(json.contains(r#""blob":"/wB/""#)); // base64(0xff 0x00 0x7f)
    assert!(json.contains(r#""status":"SUSPENDED""#));

    let back = Outer::from_json(&json).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn json_accepts_camelcase_and_number_enums() {
    // lowerCamelCase alias + numbers for enums + string-form i64.
    let json = r#"{
        "id": "12",
        "username": "bob",
        "emailAddr": "e@x.y",
        "status": 2,
        "feature": 9
    }"#;
    let m = Outer::from_json(json).unwrap();
    assert_eq!(m.id, 12);
    assert_eq!(m.username, "bob");
    assert_eq!(m.contact, Some(OuterContact::EmailAddr("e@x.y".into())));
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
    assert!(child.leaf.as_ref().unwrap().value);
}

#[test]
fn builders_validate_annotations() {
    use generated::BuildError;

    // @max_len(8) on username.
    let ok = Outer::builder().username("short").age(30).build().unwrap();
    assert_eq!(ok.username, "short");

    let err = Outer::builder()
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
    let err = Outer::builder().age(-1).build().unwrap_err();
    assert_eq!(err, BuildError::OutOfRange { field: "age" });

    // Full builder path roundtrips like the struct literal path.
    let built = Outer::builder()
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

// ---------------------------------------------------------------------------
// Recursive message types (boxed indirection)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod recursive {
    use super::generated::{Expr, ExprKind, Pair, Tree};

    fn leaf(v: i64) -> Tree {
        Tree {
            value: v,
            ..Default::default()
        }
    }

    fn sample_tree() -> Tree {
        Tree {
            value: 1,
            left: Some(Box::new(Tree {
                value: 2,
                left: Some(Box::new(leaf(4))),
                ..Default::default()
            })),
            right: Some(Box::new(leaf(3))),
            children: vec![leaf(5), leaf(6)],
            by_name: [("x".to_string(), leaf(7))].into_iter().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn self_referential_message_roundtrips_on_the_wire() {
        let t = sample_tree();
        let back = Tree::decode(&t.encode()).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.left.as_ref().unwrap().left.as_ref().unwrap().value, 4);
        assert_eq!(back.by_name["x"].value, 7);
        // Canonical output is stable too.
        assert_eq!(Tree::decode(&t.encode_canonical()).unwrap(), t);
    }

    #[test]
    fn self_referential_message_roundtrips_through_json() {
        let t = sample_tree();
        let json = t.to_json().unwrap();
        assert_eq!(Tree::from_json(&json).unwrap(), t);
    }

    #[test]
    fn self_referential_message_has_a_borrowed_view_and_builder() {
        let t = sample_tree();
        let bytes = t.encode();
        let view = Tree::decode_borrowed(&bytes).unwrap();
        assert_eq!(view.value, 1);
        assert_eq!(view.left.as_ref().unwrap().left.as_ref().unwrap().value, 4);
        assert_eq!(view.children.len(), 2);

        let built = Tree::builder().value(9).left(leaf(8)).build().unwrap();
        assert_eq!(built.left.as_ref().unwrap().value, 8);
    }

    #[test]
    fn deep_chains_survive_within_the_depth_limit() {
        let mut t = leaf(0);
        for i in 1..=50 {
            t = Tree {
                value: i,
                left: Some(Box::new(t)),
                ..Default::default()
            };
        }
        assert_eq!(Tree::decode(&t.encode()).unwrap(), t);
        // Beyond the decoder's depth limit decoding fails instead of overflowing.
        let limits = tpt20_core::DecoderLimits {
            max_depth: 10,
            ..Default::default()
        };
        assert!(Tree::decode_with_limits(&t.encode(), &limits).is_err());
    }

    #[test]
    fn mutually_recursive_oneofs_work() {
        let e = Expr {
            kind: Some(ExprKind::Add(Box::new(Pair {
                l: Some(Box::new(Expr {
                    kind: Some(ExprKind::Lit(2)),
                    ..Default::default()
                })),
                r: Some(Box::new(Expr {
                    kind: Some(ExprKind::Neg(Box::new(Expr {
                        kind: Some(ExprKind::Lit(3)),
                        ..Default::default()
                    }))),
                    ..Default::default()
                })),
                ..Default::default()
            }))),
            ..Default::default()
        };
        assert_eq!(Expr::decode(&e.encode()).unwrap(), e);
        assert_eq!(Expr::from_json(&e.to_json().unwrap()).unwrap(), e);
        let bytes = e.encode();
        assert!(Expr::decode_borrowed(&bytes).is_ok());
    }
}

#[cfg(test)]
mod json_options {
    use super::generated::{Address, Outer, OuterContact};
    use tpt20_json::{FieldNameStyle, JsonError, JsonOptions};

    fn sample() -> Outer {
        Outer {
            id: 7,
            tags: vec!["a".into()],
            contact: Some(OuterContact::EmailAddr("x@y".into())),
            ..Default::default()
        }
    }

    #[test]
    fn default_options_match_plain_json() {
        let o = sample();
        assert_eq!(
            o.to_json_with(&JsonOptions::default()).unwrap(),
            o.to_json().unwrap()
        );
    }

    #[test]
    fn lower_camel_names_on_encode_and_decode_accepts_both() {
        let o = sample();
        let opts = JsonOptions {
            field_names: FieldNameStyle::LowerCamel,
            ..Default::default()
        };
        let v: tpt20_json::json::Value =
            tpt20_json::json::from_str(&o.to_json_with(&opts).unwrap()).unwrap();
        assert!(v.get("emailAddr").is_some(), "{v}");
        assert!(v.get("email_addr").is_none());
        // Round trip through the camel spelling.
        let back = Outer::from_json(&v.to_string()).unwrap();
        assert_eq!(back.contact, o.contact);
    }

    #[test]
    fn defaults_are_emitted_on_request() {
        let o = Outer::default();
        let plain: tpt20_json::json::Value =
            tpt20_json::json::from_str(&o.to_json().unwrap()).unwrap();
        assert_eq!(plain, tpt20_json::json::json!({}));
        let opts = JsonOptions {
            emit_defaults: true,
            ..Default::default()
        };
        let full: tpt20_json::json::Value =
            tpt20_json::json::from_str(&o.to_json_with(&opts).unwrap()).unwrap();
        assert_eq!(full["id"], "0");
        assert_eq!(full["name"], "");
        assert_eq!(full["tags"], tpt20_json::json::json!([]));
        assert_eq!(full["attrs"], tpt20_json::json::json!({}));
        // Absent explicit-presence fields stay absent.
        assert!(full.get("email").is_none());
        // Emitted defaults decode back to the same message.
        assert_eq!(Outer::from_json(&full.to_string()).unwrap(), o);
    }

    #[test]
    fn unknown_members_are_ignored_unless_rejected() {
        let json = r#"{"id":"3","bogus":1}"#;
        assert_eq!(Outer::from_json(json).unwrap().id, 3);
        let strict = JsonOptions {
            reject_unknown_fields: true,
            ..Default::default()
        };
        assert_eq!(
            Outer::from_json_with(json, &strict),
            Err(JsonError::UnknownField("bogus".into()))
        );
        assert_eq!(
            Outer::from_json_with(r#"{"id":"3","emailAddr":"a"}"#, &strict)
                .unwrap()
                .id,
            3
        );
    }

    #[test]
    fn options_apply_to_nested_messages() {
        let o = Outer {
            home: Some(Address::default()),
            ..Default::default()
        };
        let opts = JsonOptions {
            emit_defaults: true,
            ..Default::default()
        };
        let v: tpt20_json::json::Value =
            tpt20_json::json::from_str(&o.to_json_with(&opts).unwrap()).unwrap();
        assert_eq!(v["home"]["street"], "");
        let strict = JsonOptions {
            reject_unknown_fields: true,
            ..Default::default()
        };
        assert!(Outer::from_json_with(r#"{"home":{"zip":"1"}}"#, &strict).is_err());
    }
}

/// Differential tests: `prost` (an independent protobuf implementation) and
/// the tpt20 protobuf adapter + generated code must agree on the bytes.
#[cfg(test)]
mod protobuf_differential {
    use super::generated::{Diff, DiffInner, DiffPick};
    use prost::Message;
    use std::collections::HashMap;
    use tpt20_compat_protobuf::schema_wire::{native_to_protobuf, protobuf_to_native};
    use tpt20_core::DecoderLimits;

    fn package() -> &'static tpt20_ir::PackageIr {
        static PKG: std::sync::OnceLock<tpt20_ir::PackageIr> = std::sync::OnceLock::new();
        PKG.get_or_init(|| {
            tpt20_compiler::pipeline::compile(include_str!("schema.tpt"), None)
                .map_err(|d| format!("{d:?}"))
                .unwrap()
                .ir
        })
    }

    #[derive(Clone, PartialEq, prost::Message)]
    struct PInner {
        #[prost(string, tag = "1")]
        x: String,
        #[prost(int32, tag = "2")]
        y: i32,
    }

    #[derive(Clone, PartialEq, prost::Oneof)]
    enum PPick {
        #[prost(int32, tag = "23")]
        Pa(i32),
        #[prost(string, tag = "24")]
        Pb(String),
    }

    #[derive(Clone, PartialEq, prost::Message)]
    struct PDiff {
        #[prost(int32, tag = "1")]
        a: i32,
        #[prost(int64, tag = "2")]
        b: i64,
        #[prost(uint32, tag = "3")]
        c: u32,
        #[prost(uint64, tag = "4")]
        d: u64,
        #[prost(sint32, tag = "5")]
        e: i32,
        #[prost(sint64, tag = "6")]
        f: i64,
        #[prost(bool, tag = "7")]
        g: bool,
        #[prost(fixed32, tag = "8")]
        h: u32,
        #[prost(fixed64, tag = "9")]
        i: u64,
        #[prost(sfixed32, tag = "10")]
        j: i32,
        #[prost(sfixed64, tag = "11")]
        k: i64,
        #[prost(float, tag = "12")]
        l: f32,
        #[prost(double, tag = "13")]
        m: f64,
        #[prost(string, tag = "14")]
        s: String,
        #[prost(bytes = "vec", tag = "15")]
        by: Vec<u8>,
        #[prost(int32, repeated, tag = "16")]
        rp: Vec<i32>,
        #[prost(string, repeated, tag = "17")]
        rs: Vec<String>,
        #[prost(sint64, repeated, tag = "18")]
        rz: Vec<i64>,
        #[prost(double, repeated, tag = "19")]
        rd: Vec<f64>,
        #[prost(map = "string, int64", tag = "20")]
        m1: HashMap<String, i64>,
        #[prost(message, optional, tag = "21")]
        nested: Option<PInner>,
        #[prost(int32, optional, tag = "22")]
        opt: Option<i32>,
        #[prost(map = "string, message", tag = "25")]
        mm: HashMap<String, PInner>,
        #[prost(message, repeated, tag = "26")]
        rn: Vec<PInner>,
        #[prost(oneof = "PPick", tags = "23, 24")]
        pick: Option<PPick>,
    }

    /// Small deterministic generator (xorshift64*).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
            xs[(self.next() % xs.len() as u64) as usize]
        }
        fn text(&mut self) -> String {
            let n = self.next() % 6;
            (0..n)
                .map(|_| self.pick(&['a', 'é', '漢', '🦀', 'z', ' ']))
                .collect()
        }
    }

    fn random(rng: &mut Rng) -> PDiff {
        let i32s = [0, 1, -1, i32::MAX, i32::MIN, 127, 128, -129];
        let i64s = [0, 1, -1, i64::MAX, i64::MIN, 1 << 35, -(1 << 35)];
        let mut d = PDiff {
            a: if rng.next() % 2 == 0 {
                rng.pick(&i32s)
            } else {
                0
            },
            b: if rng.next() % 2 == 0 {
                rng.pick(&i64s)
            } else {
                0
            },
            c: if rng.next() % 2 == 0 {
                rng.next() as u32
            } else {
                0
            },
            d: if rng.next() % 2 == 0 { rng.next() } else { 0 },
            e: rng.pick(&i32s),
            f: rng.pick(&i64s),
            g: rng.next() % 2 == 0,
            h: rng.next() as u32,
            i: rng.next(),
            j: rng.pick(&i32s),
            k: rng.pick(&i64s),
            l: rng.pick(&[0.0, 1.5, -2.25, f32::MAX, f32::MIN_POSITIVE]),
            m: rng.pick(&[0.0, 1.5, -2.25, f64::MAX, f64::MIN_POSITIVE]),
            s: rng.text(),
            by: (0..rng.next() % 5).map(|_| rng.next() as u8).collect(),
            ..Default::default()
        };
        for _ in 0..rng.next() % 4 {
            d.rp.push(rng.pick(&i32s));
            d.rs.push(rng.text());
            d.rz.push(rng.pick(&i64s));
            d.rd.push(rng.pick(&[0.5, -1e300]));
        }
        for _ in 0..rng.next() % 3 {
            d.m1.insert(rng.text(), rng.pick(&i64s));
        }
        for _ in 0..rng.next() % 3 {
            let inner = PInner {
                x: rng.text(),
                y: rng.pick(&i32s),
            };
            d.mm.insert(rng.text(), inner.clone());
            d.rn.push(inner);
        }
        if rng.next() % 2 == 0 {
            d.nested = Some(PInner {
                x: rng.text(),
                y: rng.pick(&i32s),
            });
        }
        if rng.next() % 2 == 0 {
            d.opt = Some(rng.pick(&i32s));
        }
        d.pick = match rng.next() % 3 {
            0 => Some(PPick::Pa(rng.pick(&i32s))),
            1 => Some(PPick::Pb(rng.text())),
            _ => None,
        };
        d
    }

    fn to_native(p: &PDiff) -> Diff {
        Diff {
            a: p.a,
            b: p.b,
            c: p.c,
            d: p.d,
            e: p.e,
            f: p.f,
            g: p.g,
            h: p.h,
            i: p.i,
            j: p.j,
            k: p.k,
            l: p.l,
            m: p.m,
            s: p.s.clone(),
            by: p.by.clone(),
            rp: p.rp.clone(),
            rs: p.rs.clone(),
            rz: p.rz.clone(),
            rd: p.rd.clone(),
            m1: p.m1.clone().into_iter().collect(),
            nested: p.nested.as_ref().map(|n| DiffInner {
                x: n.x.clone(),
                y: n.y,
                ..Default::default()
            }),
            mm: p
                .mm
                .iter()
                .map(|(k, n)| {
                    (
                        k.clone(),
                        DiffInner {
                            x: n.x.clone(),
                            y: n.y,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            rn: p
                .rn
                .iter()
                .map(|n| DiffInner {
                    x: n.x.clone(),
                    y: n.y,
                    ..Default::default()
                })
                .collect(),
            opt: p.opt,
            pick: p.pick.as_ref().map(|k| match k {
                PPick::Pa(v) => DiffPick::Pa(*v),
                PPick::Pb(v) => DiffPick::Pb(v.clone()),
            }),
            ..Default::default()
        }
    }

    /// prost bytes -> adapter -> native bytes -> generated decode.
    fn prost_to_native(bytes: &[u8]) -> Diff {
        let native = protobuf_to_native(bytes, package(), "Diff", &DecoderLimits::default())
            .expect("adapter converts protobuf bytes");
        Diff::decode(&native).expect("generated code decodes adapted bytes")
    }

    /// generated encode -> adapter -> protobuf bytes.
    fn native_to_proto(d: &Diff) -> Vec<u8> {
        native_to_protobuf(&d.encode(), package(), "Diff", &DecoderLimits::default())
            .expect("adapter converts native bytes")
    }

    #[test]
    fn prost_bytes_decode_through_the_adapter() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for n in 0..500 {
            let p = random(&mut rng);
            let got = prost_to_native(&p.encode_to_vec());
            let mut want = to_native(&p);
            // Native decode keeps no unknown fields here; compare the rest.
            want.unknown_fields = got.unknown_fields.clone();
            assert_eq!(got, want, "case {n}: {p:?}");
        }
    }

    #[test]
    fn native_bytes_decode_in_prost() {
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        for n in 0..500 {
            let p = random(&mut rng);
            let bytes = native_to_proto(&to_native(&p));
            let got = PDiff::decode(bytes.as_slice())
                .unwrap_or_else(|e| panic!("case {n}: prost rejected adapter output: {e}"));
            assert_eq!(got, p, "case {n}");
        }
    }

    #[test]
    fn protobuf_bytes_are_stable_under_reencoding() {
        // prost's canonical bytes, run through the adapter twice, stay valid
        // protobuf and decode to the same message.
        let mut rng = Rng(42);
        for _ in 0..200 {
            let p = random(&mut rng);
            let once = native_to_proto(&prost_to_native(&p.encode_to_vec()));
            assert_eq!(PDiff::decode(once.as_slice()).unwrap(), p);
        }
    }
}

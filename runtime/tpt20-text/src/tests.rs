use super::*;
use tpt20_core::{Field, Value, WireClass};

const SCHEMA: &str = r#"
package text.v1;

message Address {
  1: street string;
  2: city string?;
}

message Outer {
  1: id int64;
  2: name string;
  3: email string?;
  4: tags repeated string;
  5: scores repeated int64;
  6: attrs map<string, string>;
  7: counts map<int64, Address>;
  oneof contact {
    10: email_addr string;
    11: phone string;
    12: addr Address;
  }
  enum Status {
    ACTIVE = 0;
    INACTIVE = 1;
  }
  13: status Status;
  open enum Feature {
    NONE = 0;
    BETA = 1;
  }
  14: feature Feature;
  15: home Address;
  16: blob bytes;
  17: ratio float64;
  18: flags repeated uint32;
  19: zigzag sint64;
  20: small int32;
  21: on bool;
  22: f32 float32;
  23: fx fixed32;
  24: sfx sfixed64;
}
"#;

fn descriptor() -> Descriptor {
    let compiled = tpt20_compiler::compile(SCHEMA, Some("text.tpt")).unwrap();
    Descriptor::new(compiled.ir)
}

fn canon(raw: &RawMessage) -> Vec<u8> {
    let mut r = raw.clone();
    r.fields.sort_by_key(|f| f.field_id);
    r.encode().unwrap()
}

#[test]
fn spec_example_prints_exactly() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let mut raw = RawMessage::new();
    raw.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    raw.push(Field::new(2, WireClass::Len, Value::Len(b"Ada".to_vec())));
    raw.push(Field::new(
        3,
        WireClass::Len,
        Value::Len(b"ada@example.com".to_vec()),
    ));
    assert_eq!(
        fmt.print("Outer", &raw).unwrap(),
        "id: 42\nname: \"Ada\"\nemail: \"ada@example.com\"\n"
    );
}

const FULL: &str = r#"
# a comment
id: -5
name: "Ada \"the\" \n\x01é"
email: "ada@example.com"
tags: "a"
tags: "b"
scores: [1, -2, 300]
attrs { key: "z" value: "last" }
attrs { key: "a" value: "first" }
counts { key: 2 value { street: "two" } }
counts { key: -1 value { street: "neg" city: "X" } }
addr { street: "1 Way" }
status: INACTIVE
feature: 7
home { street: "h" }
blob: "\xff\x00abc"
ratio: -2.5
flags: 1
flags: 4294967295
zigzag: -9223372036854775808
small: -2147483648
on: true
f32: 0.1
fx: 0x10
sfx: -3
"#;

#[test]
fn full_roundtrip_is_stable_and_deterministic() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let raw = fmt.parse("Outer", FULL).unwrap();
    let text1 = fmt.print("Outer", &raw).unwrap();

    // Printing is sorted by field id and maps by key.
    let id_pos = text1.find("id: -5").unwrap();
    let name_pos = text1.find("name:").unwrap();
    assert!(id_pos < name_pos);
    assert!(text1.find("key: \"a\"").unwrap() < text1.find("key: \"z\"").unwrap());
    assert!(text1.find("key: -1").unwrap() < text1.find("key: 2").unwrap());
    assert!(text1.contains("status: INACTIVE"));
    assert!(text1.contains("feature: 7"));
    assert!(text1.contains("f32: 0.1\n"));
    assert!(text1.contains("home {\n  street: \"h\"\n}"));

    // text → raw → text → raw is a fixed point (canonical bytes equal).
    let raw2 = fmt.parse("Outer", &text1).unwrap();
    assert_eq!(fmt.print("Outer", &raw2).unwrap(), text1);
    // Only map-entry order may differ from the hand-written input.
    assert_eq!(raw.fields.len(), raw2.fields.len());

    // And it survives the real wire format.
    let bytes = fmt.parse_to_bytes("Outer", FULL).unwrap();
    assert_eq!(fmt.print_bytes("Outer", &bytes).unwrap(), text1);
}

#[test]
fn packed_repeated_fields_print_one_per_line() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let packed = tpt20_core::scalar::encode_packed_varints(&[1, 2, 3]);
    let mut raw = RawMessage::new();
    raw.push(Field::new(18, WireClass::Len, packed));
    assert_eq!(
        fmt.print("Outer", &raw).unwrap(),
        "flags: 1\nflags: 2\nflags: 3\n"
    );
}

#[test]
fn oneof_prints_only_the_winner_and_parser_rejects_two() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let mut raw = RawMessage::new();
    raw.push(Field::new(
        10,
        WireClass::Len,
        Value::Len(b"first".to_vec()),
    ));
    raw.push(Field::new(
        11,
        WireClass::Len,
        Value::Len(b"second".to_vec()),
    ));
    assert_eq!(fmt.print("Outer", &raw).unwrap(), "phone: \"second\"\n");

    assert_eq!(
        fmt.parse("Outer", "phone: \"a\" email_addr: \"b\""),
        Err(TextError::OneofConflict("contact".into()))
    );
}

#[test]
fn nested_message_names_and_scopes_resolve() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let raw = fmt.parse("Address", "street: \"s\" city: \"c\"").unwrap();
    assert_eq!(
        fmt.print("Address", &raw).unwrap(),
        "street: \"s\"\ncity: \"c\"\n"
    );
    assert_eq!(
        fmt.parse("Nope", ""),
        Err(TextError::UnknownMessage("Nope".into()))
    );
}

#[test]
fn separators_and_colon_before_message_are_optional() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let a = fmt
        .parse("Outer", "id: 1, name: \"x\"; home: { street: \"s\" }")
        .unwrap();
    let b = fmt
        .parse("Outer", "id: 1\nname: \"x\"\nhome { street: \"s\" }")
        .unwrap();
    assert_eq!(canon(&a), canon(&b));
}

#[test]
fn parse_errors_are_reported_not_panicked() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let cases: &[(&str, fn(&TextError) -> bool)] = &[
        ("bogus: 1", |e| matches!(e, TextError::UnknownField { .. })),
        ("id: 1 id: 2", |e| matches!(e, TextError::DuplicateField(_))),
        ("id: \"x\"", |e| matches!(e, TextError::TypeMismatch { .. })),
        ("small: 2147483648", |e| {
            matches!(e, TextError::OutOfRange(_))
        }),
        ("flags: -1", |e| matches!(e, TextError::OutOfRange(_))),
        ("status: NOPE", |e| {
            matches!(e, TextError::InvalidEnum { .. })
        }),
        ("status: 9", |e| matches!(e, TextError::InvalidEnum { .. })),
        ("name: \"\\xff\"", |e| {
            matches!(e, TextError::InvalidUtf8(_))
        }),
        ("name: \"open", |e| matches!(e, TextError::Syntax { .. })),
        ("home { street: \"s\"", |e| {
            matches!(e, TextError::Syntax { .. })
        }),
        ("id 1", |e| matches!(e, TextError::Syntax { .. })),
        ("attrs { key: \"k\" }", |e| {
            matches!(e, TextError::MalformedMapEntry(_))
        }),
        ("on: 1", |e| matches!(e, TextError::TypeMismatch { .. })),
        ("}", |e| matches!(e, TextError::Syntax { .. })),
        ("id: 1 @", |e| matches!(e, TextError::Syntax { .. })),
    ];
    for (text, check) in cases {
        let err = fmt.parse("Outer", text).expect_err(text);
        assert!(check(&err), "{text:?} gave {err:?}");
    }
    // Open enums accept unknown numbers.
    assert!(fmt.parse("Outer", "feature: 99").is_ok());
}

#[test]
fn depth_and_size_limits_hold() {
    let d = descriptor();
    let mut fmt = TextFormat::new(&d);
    fmt.max_depth = 3;
    let nested = "home { street: \"s\" }";
    assert!(fmt.parse("Outer", nested).is_ok());
    // A pathologically deep brace nest on a message-typed field must error,
    // not overflow the stack.
    let bomb = "home {".repeat(10_000);
    assert!(fmt.parse("Outer", &bomb).is_err());
    fmt.max_text_bytes = 8;
    assert_eq!(
        fmt.parse("Outer", "id: 1234567890"),
        Err(TextError::LimitExceeded("text size"))
    );
}

#[test]
fn arbitrary_garbage_never_panics() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let mut seed = 0x1234_5678_u64;
    let alphabet: Vec<char> = "idnamehomestreet{}[]:;,\"'\\ \n#-+.0123456789xe_"
        .chars()
        .collect();
    for _ in 0..3000 {
        let mut s = String::new();
        for _ in 0..(seed % 40) {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s.push(alphabet[(seed >> 33) as usize % alphabet.len()]);
        }
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let _ = fmt.parse("Outer", &s);
    }
}

#[test]
fn wire_mismatch_is_an_error() {
    let d = descriptor();
    let fmt = TextFormat::new(&d);
    let mut raw = RawMessage::new();
    raw.push(Field::new(1, WireClass::Len, Value::Len(vec![1])));
    assert_eq!(
        fmt.print("Outer", &raw),
        Err(TextError::WireMismatch("id".into()))
    );
}

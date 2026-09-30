//! `.proto` import: editions, extensions, scoping and robustness.

use tpt20_compat_protobuf::{lex_proto, lower, lower_with_report, parse_proto, ProtoError};
use tpt20_ir::{FieldLabelIr, PackageIr, Presence};

fn import(src: &str) -> Result<PackageIr, ProtoError> {
    lower(parse_proto(lex_proto(src)?)?)
}

fn field<'a>(pkg: &'a PackageIr, msg: &str, name: &str) -> &'a tpt20_ir::FieldIr {
    pkg.messages
        .iter()
        .find(|m| m.name == msg)
        .unwrap()
        .fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no field {name}"))
}

#[test]
fn proto3_presence_and_enum_openness() {
    let pkg = import(
        r#"syntax = "proto3";
package p;
enum E { A = 0; B = 1; }
message M { int32 a = 1; optional int32 b = 2; E e = 3; repeated int32 r = 4; }"#,
    )
    .unwrap();
    assert_eq!(field(&pkg, "M", "a").presence, Presence::Implicit);
    assert_eq!(field(&pkg, "M", "b").presence, Presence::Explicit);
    assert!(pkg.enums[0].open);
}

#[test]
fn proto2_is_explicit_and_closed() {
    let pkg = import(
        r#"syntax = "proto2";
package p;
enum E { A = 0; }
message M { optional int32 a = 1; required string r = 2; }"#,
    )
    .unwrap();
    assert_eq!(field(&pkg, "M", "a").presence, Presence::Explicit);
    let r = field(&pkg, "M", "r");
    assert_eq!(r.presence, Presence::Explicit);
    assert!(r.annotations.iter().any(|a| a.name == "proto_required"));
    assert!(!pkg.enums[0].open);
}

#[test]
fn editions_2023_defaults_and_features() {
    let pkg = import(
        r#"edition = "2023";
package p;
enum Open { A = 0; }
enum Closed { option features.enum_type = CLOSED; B = 0; }
message M {
  int32 explicit_by_default = 1;
  int32 implicit_here = 2 [features.field_presence = IMPLICIT];
  message Inner {
    option features.field_presence = IMPLICIT;
    int32 x = 1;
    int32 y = 2 [features.field_presence = EXPLICIT];
  }
}"#,
    )
    .unwrap();
    assert_eq!(
        field(&pkg, "M", "explicit_by_default").presence,
        Presence::Explicit
    );
    assert_eq!(
        field(&pkg, "M", "implicit_here").presence,
        Presence::Implicit
    );
    let inner = &pkg.messages[0].messages[0];
    assert_eq!(inner.fields[0].presence, Presence::Implicit);
    assert_eq!(inner.fields[1].presence, Presence::Explicit);
    assert!(pkg.enums[0].open);
    assert!(!pkg.enums[1].open);
}

#[test]
fn file_level_feature_applies_to_everything() {
    let pkg = import(
        r#"edition = "2023";
option features.field_presence = IMPLICIT;
message M { int32 a = 1; }"#,
    )
    .unwrap();
    assert_eq!(field(&pkg, "M", "a").presence, Presence::Implicit);
}

#[test]
fn unsupported_editions_and_features_are_errors() {
    assert!(import("edition = \"2099\";\nmessage M {}").is_err());
    assert!(import("syntax = \"proto3\";\nedition = \"2023\";").is_err());
    assert!(import(
        "edition = \"2023\";\nmessage M { M m = 1 [features.message_encoding = DELIMITED]; }"
    )
    .is_err());
    assert!(import(
        "syntax = \"proto2\";\nmessage M { optional group G = 1 { optional int32 x = 2; } }"
    )
    .is_err());
}

#[test]
fn extensions_merge_into_local_extendees() {
    let (pkg, report) = lower_with_report(
        parse_proto(
            lex_proto(
                r#"syntax = "proto2";
package p;
message Base { optional int32 a = 1; extensions 100 to 199; }
message Holder {
  extend Base { optional string nested_ext = 101; repeated int64 many = 102; }
}
extend Base { optional bool flag = 100; }
extend google.protobuf.FieldOptions { optional string my_option = 50000; }
"#,
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let base = &pkg.messages[0];
    let names: Vec<_> = base
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.id))
        .collect();
    assert_eq!(
        names,
        vec![("a", 1), ("flag", 100), ("nested_ext", 101), ("many", 102)]
    );
    assert!(matches!(base.fields[3].label, FieldLabelIr::Repeated(_)));
    assert_eq!(base.fields[1].presence, Presence::Explicit);
    assert_eq!(
        report.dropped_extensions,
        vec!["google.protobuf.FieldOptions.my_option".to_string()]
    );
}

#[test]
fn extension_conflicts_are_rejected() {
    // id already used
    let e = import(
        r#"syntax = "proto2";
message B { optional int32 a = 100; extensions 100 to 199; }
extend B { optional int32 b = 100; }"#,
    )
    .unwrap_err();
    assert_eq!(e, ProtoError::ExtensionConflict(100));
    // outside the declared ranges
    let e = import(
        r#"syntax = "proto2";
message B { extensions 100 to 199; }
extend B { optional int32 b = 7; }"#,
    )
    .unwrap_err();
    assert_eq!(e, ProtoError::ExtensionConflict(7));
    // name clash
    let e = import(
        r#"syntax = "proto2";
message B { optional int32 a = 1; extensions 100 to max; }
extend B { optional int32 a = 100; }"#,
    )
    .unwrap_err();
    assert_eq!(e, ProtoError::DuplicateField("a".into()));
}

#[test]
fn types_resolve_with_protobuf_scoping() {
    let pkg = import(
        r#"syntax = "proto3";
package acme.v1;
message Outer {
  message Inner { int32 x = 1; }
  Inner a = 1;
  Outer.Inner b = 2;
  .acme.v1.Outer.Inner c = 3;
  acme.v1.Top d = 4;
  Status s = 5;
  enum Status { UNKNOWN = 0; }
}
message Top {}
message Other { Outer.Inner i = 1; }"#,
    )
    .unwrap();
    let path = |m: &str, f: &str| match &field(&pkg, m, f).label {
        FieldLabelIr::Singular(t) => t.path.join("."),
        _ => unreachable!(),
    };
    assert_eq!(path("Outer", "a"), "Outer.Inner");
    assert_eq!(path("Outer", "b"), "Outer.Inner");
    assert_eq!(path("Outer", "c"), "Outer.Inner");
    assert_eq!(path("Outer", "d"), "Top");
    assert_eq!(path("Outer", "s"), "Outer.Status");
    assert_eq!(path("Other", "i"), "Outer.Inner");
}

#[test]
fn contextual_keywords_are_valid_names() {
    let pkg = import(
        r#"syntax = "proto3";
message M {
  int32 max = 1;
  string stream = 2;
  bool default = 3;
  bytes packed = 4;
  int32 option = 5;
  int32 map = 6;
  int32 to = 7;
  enum E { public = 0; weak = 1; }
  E e = 8;
}"#,
    )
    .unwrap();
    let names: Vec<_> = pkg.messages[0]
        .fields
        .iter()
        .map(|f| f.name.clone())
        .collect();
    assert_eq!(
        names,
        ["max", "stream", "default", "packed", "option", "map", "to", "e"]
    );
    assert_eq!(pkg.messages[0].enums[0].values[0].name, "public");
}

#[test]
fn options_services_and_reserved_ranges_parse() {
    let pkg = import(
        r#"syntax = "proto3";
package s;
option java_package = "x.y";
option (custom.opt).sub = { a: 1, b: { c: 2 } };
message M {
  reserved 1 to 3, 9 to max;
  reserved "old";
  int32 f = 4 [deprecated = true, default = -1, (my.ext) = "v"];
}
enum E {
  option allow_alias = true;
  reserved 5, 6;
  A = 0;
  B = 0;
  C = 1 [deprecated = true];
}
service Svc {
  option deprecated = true;
  rpc A(M) returns (M);
  rpc B(stream M) returns (stream M) { option idempotency_level = IDEMPOTENT; }
  rpc C(.s.M) returns (s.M) {}
}"#,
    )
    .unwrap();
    let m = &pkg.messages[0];
    assert_eq!(m.reserved.len(), 2);
    assert!(m.fields[0]
        .annotations
        .iter()
        .any(|a| a.name == "deprecated"));
    let e = &pkg.enums[0];
    assert!(!e.values[0].alias && e.values[1].alias && !e.values[2].alias);
    let svc = &pkg.services[0];
    assert_eq!(svc.methods.len(), 3);
    assert!(svc.methods[1].request_streaming && svc.methods[1].response_streaming);
    assert_eq!(svc.methods[2].request.path, vec!["M".to_string()]);
    assert_eq!(svc.methods[2].response.path, vec!["M".to_string()]);
    assert!(svc.annotations.iter().any(|a| a.name == "deprecated"));
}

#[test]
fn scalar_and_map_lowering_uses_ir_type_names() {
    let pkg = import(
        r#"syntax = "proto3";
message M { double d = 1; float f = 2; map<int64, M> m = 3; }"#,
    )
    .unwrap();
    let ty = |n: &str| match &field(&pkg, "M", n).label {
        FieldLabelIr::Singular(t) => t.path.join("."),
        _ => unreachable!(),
    };
    assert_eq!(ty("d"), "float64");
    assert_eq!(ty("f"), "float32");
    match &field(&pkg, "M", "m").label {
        FieldLabelIr::Map { key, value } => {
            assert_eq!(key.path, ["int64"]);
            assert_eq!(value.path, ["M"]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn hostile_inputs_do_not_overflow_or_panic() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let deep = format!(
                "syntax = \"proto3\";\n{}{}",
                "message M {\n".repeat(50_000),
                "}\n".repeat(50_000)
            );
            assert!(import(&deep).is_err());
            for junk in [
                "",
                "message",
                "message M {",
                "syntax =",
                "extend",
                "extend X {",
                "message M { extensions 1 to ; }",
                "message M { reserved 1 to max to 2; }",
                "service S { rpc",
                "option (x.y",
                "message M { int32 a = 99999999999; }",
                &"{".repeat(100_000),
                &"(".repeat(100_000),
            ] {
                let _ = import(junk);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

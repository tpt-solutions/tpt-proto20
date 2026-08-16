//! Integration tests for the tpt20 compiler pipeline, semantic analysis,
//! compatibility detection, and schema history manifest.

use tpt20_compiler::compat::ChangeClass;
use tpt20_compiler::manifest::SchemaHistoryManifest;
use tpt20_compiler::{check, compile, diff_sources};

const EXAMPLE: &str = r#"
    package user.v1;

    message User {
        1: id int64;
        2: name string;
        3: email string?;
        4: repeated tags string;
        5: map<string, string> attributes;
        oneof contact {
            10: email_addr string;
            11: phone string;
        }
        7: status Status;
    }

    enum Status {
        UNKNOWN = 0;
        ACTIVE = 1;
        INACTIVE = 2;
    }

    open enum Feature {
        NONE = 0;
        BETA = 1;
    }

    service UserService {
        GetUser(GetUserRequest) returns (User);
        Subscribe(stream GetUserRequest) returns (stream User);
    }

    message GetUserRequest {
        1: id int64;
    }
"#;

#[test]
fn compiles_example_schema() {
    let out = compile(EXAMPLE, Some("user.v1.tpt")).expect("example should compile");
    assert_eq!(out.ir.name.as_deref(), Some("user.v1"));
    assert_eq!(out.ir.messages.len(), 2);
    assert!(!out.fingerprint.is_empty());
    // No errors.
    assert!(out
        .diagnostics
        .iter()
        .all(|d| d.severity != tpt20_compiler::Severity::Error));
}

#[test]
fn detects_duplicate_field_id() {
    let src = r#"
        message M {
            1: a int32;
            1: b int32;
        }
    "#;
    let diags = check(src, Some("m.tpt"));
    assert!(diags.iter().any(|d| d.code == "E0001"));
}

#[test]
fn rejects_bytes_map_key() {
    let src = r#"
        message M {
            1: map<bytes, string> bad;
        }
    "#;
    let diags = check(src, Some("m.tpt"));
    assert!(diags.iter().any(|d| d.code == "E0007"));
}

#[test]
fn flags_unknown_scalar_type() {
    let src = r#"
        message M {
            1: x widget;
        }
    "#;
    let diags = check(src, Some("m.tpt"));
    assert!(diags.iter().any(|d| d.code == "E0010"));
}

#[test]
fn oneof_member_must_be_singular() {
    let src = r#"
        message M {
            oneof o {
                1: repeated a string;
            }
        }
    "#;
    let diags = check(src, Some("m.tpt"));
    assert!(diags.iter().any(|d| d.code == "E0005"));
}

#[test]
fn compat_added_field_is_safe() {
    let old = "message User { 1: id int64; }";
    let new = "message User { 1: id int64; 2: name string; }";
    let changes = diff_sources(old, new).unwrap();
    let added = changes
        .iter()
        .find(|c| c.message.contains("added field 2"))
        .unwrap();
    assert_eq!(added.class, ChangeClass::Safe);
}

#[test]
fn compat_removed_field_without_reservation_is_breaking() {
    let old = "message User { 1: id int64; 2: name string; }";
    let new = "message User { 1: id int64; }";
    let changes = diff_sources(old, new).unwrap();
    let removed = changes
        .iter()
        .find(|c| c.message.contains("removed field 2"))
        .unwrap();
    assert_eq!(removed.class, ChangeClass::Breaking);
    assert!(removed.message.contains("without reservation"));
}

#[test]
fn compat_removed_field_with_reservation_is_safe() {
    let old = "message User { 1: id int64; 2: name string; }";
    let new = "message User { 1: id int64; reserved 2; }";
    let changes = diff_sources(old, new).unwrap();
    let removed = changes
        .iter()
        .find(|c| c.message.contains("removed field") && c.message.contains("reserved"))
        .unwrap();
    assert_eq!(removed.class, ChangeClass::Safe);
}

#[test]
fn compat_renamed_field_is_warning() {
    let old = "message User { 1: username string; }";
    let new = "message User { 1: login string; }";
    let changes = diff_sources(old, new).unwrap();
    let renamed = changes
        .iter()
        .find(|c| c.message.contains("renamed field"))
        .unwrap();
    assert_eq!(renamed.class, ChangeClass::Warning);
}

#[test]
fn compat_changed_field_type_is_breaking() {
    let old = "message User { 1: id int64; }";
    let new = "message User { 1: id string; }";
    let changes = diff_sources(old, new).unwrap();
    let changed = changes
        .iter()
        .find(|c| c.message.contains("changed type"))
        .unwrap();
    assert_eq!(changed.class, ChangeClass::Breaking);
}

#[test]
fn compat_added_enum_value_in_open_enum_is_safe() {
    let old = "open enum E { A = 0; }";
    let new = "open enum E { A = 0; B = 1; }";
    let changes = diff_sources(old, new).unwrap();
    let added = changes
        .iter()
        .find(|c| c.message.contains("added enum value"))
        .unwrap();
    assert_eq!(added.class, ChangeClass::Safe);
}

#[test]
fn diff_render_matches_cli_format() {
    let old = "message User { 1: id int64; }";
    let new = "message User { 1: id int64; 2: name string; }";
    let changes = diff_sources(old, new).unwrap();
    let report = tpt20_compiler::render_report(&changes);
    assert!(report.contains("SAFE     added field 2 name"));
}

#[test]
fn manifest_json_roundtrip() {
    let mut m = SchemaHistoryManifest::new(Some("user.v1".to_string()));
    m.record_version("user.v1", "abc123", "strict");
    m.record_policy("strict");
    m.record_reserved(
        "User",
        vec![tpt20_compiler::manifest::ReservedId::Range(100, 200)],
        vec!["old".to_string()],
    );
    m.record_deprecation("User.legacy", "2026-01-01");
    let json = m.to_json().unwrap();
    let back = SchemaHistoryManifest::from_json(&json).unwrap();
    assert_eq!(m, back);
}

#[test]
fn unknown_annotation_is_warning() {
    let src = r#"
        message M {
            @custom(1) 1: x int32;
        }
    "#;
    let diags = check(src, Some("m.tpt"));
    assert!(diags
        .iter()
        .any(|d| d.code == "E0009" && d.severity == tpt20_compiler::Severity::Warning));
}

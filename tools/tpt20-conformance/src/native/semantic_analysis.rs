use tpt20_compiler::pipeline::check as semantic_check;

#[test]
fn accepts_valid_schema() {
    let src = r#"package "test.v1"
message User {
  id: int64
  name: string
}"#;
    let diags = semantic_check(src, None);
    let errors: Vec<_> = diags.iter().filter(|d| d.severity == tpt20_compiler::diagnostics::Severity::Error).collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

#[test]
fn detects_duplicate_field_ids() {
    let src = r#"message Foo {
  a: int64 = 1
  b: int64 = 1
}"#;
    let diags = semantic_check(src, None);
    assert!(
        diags.iter().any(|d| d.code == "E0001"),
        "expected duplicate field id diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn detects_duplicate_field_names() {
    let src = r#"message Foo {
  name: int64
  name: string
}"#;
    let diags = semantic_check(src, None);
    assert!(
        diags.iter().any(|d| d.code == "E0003"),
        "expected duplicate name diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn detects_reserved_id_reuse() {
    let src = r#"message Foo {
  reserved 1
  id: int64 = 1
}"#;
    let diags = semantic_check(src, None);
    assert!(
        diags.iter().any(|d| d.code == "E0006"),
        "expected reserved id reuse diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn accepts_known_scalars() {
    for scalar in tpt20_compiler::semantic::KNOWN_SCALARS.iter() {
        let src = format!(r#"message Foo {{
  value: {}
}}"#, scalar);
        let diags = semantic_check(&src, None);
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == tpt20_compiler::diagnostics::Severity::Error).collect();
        assert!(errors.is_empty(), "scalar {} should be accepted, got: {:?}", scalar, errors);
    }
}

#[test]
fn annotation_registry() {
    use tpt20_compiler::semantic::AnnotationRegistry;
    let reg = AnnotationRegistry::builtins();
    assert!(reg.is_known("max_len"));
    assert!(reg.is_known("deprecated"));
    assert!(!reg.is_known("custom_annotation"));

    let mut reg = AnnotationRegistry::builtins();
    reg.register("custom_annotation");
    assert!(reg.is_known("custom_annotation"));
}

#[test]
fn detects_unknown_scalar_type() {
    let src = r#"message Foo {
  value: notatype
}"#;
    let diags = semantic_check(src, None);
    assert!(
        diags.iter().any(|d| d.code == "E0010"),
        "expected unknown type diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn detects_map_key_bytes_forbidden() {
    let src = r#"message Foo {
  data: map<bytes, string>
}"#;
    let diags = semantic_check(src, None);
    assert!(
        diags.iter().any(|d| d.code == "E0007"),
        "expected map key diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn detects_duplicate_enum_values() {
    let src = r#"enum State {
  ACTIVE = 0
  ACTIVE = 0
}"#;
    let diags = semantic_check(src, None);
    assert!(
        diags.iter().any(|d| d.code == "E0002"),
        "expected duplicate enum value diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn rejects_required_keyword() {
    let src = "message M { 1: required x int32; }";
    let diags = semantic_check(src, None);
    assert!(
        diags.iter().any(|d| d.code == "E0011"),
        "expected required keyword diagnostic, got: {:?}",
        diags
    );
}

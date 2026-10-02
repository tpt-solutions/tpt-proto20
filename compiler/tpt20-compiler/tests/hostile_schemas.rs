//! Malicious-schema corpus: hostile inputs must yield diagnostics (or succeed),
//! never a panic or a stack overflow.

use tpt20_compiler::pipeline::{check, compile};

fn run_on_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    // A deliberately small stack makes recursion bugs show up reliably.
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .expect("compiler panicked or overflowed on hostile input");
}

fn nested_messages(depth: usize) -> String {
    let mut s = String::from("package x.v1;\n");
    for i in 0..depth {
        s.push_str(&format!("message M{i} {{\n"));
    }
    s.push_str(&"}\n".repeat(depth));
    s
}

#[test]
fn deeply_nested_messages_are_rejected_not_overflowed() {
    run_on_big_stack(|| {
        let diags = check(&nested_messages(100_000), None);
        assert!(!diags.is_empty());
    });
}

#[test]
fn moderately_nested_messages_compile() {
    run_on_big_stack(|| {
        assert!(compile(&nested_messages(60), None).is_ok());
    });
}

#[test]
fn deeply_nested_generics_do_not_overflow() {
    run_on_big_stack(|| {
        let mut ty = String::from("int32");
        for _ in 0..50_000 {
            ty = format!("map<string, {ty}>");
        }
        let src = format!("package x.v1;\nmessage M {{ 1: f {ty}; }}\n");
        let _ = check(&src, None);
    });
}

#[test]
fn huge_schema_is_handled() {
    run_on_big_stack(|| {
        let mut s = String::from("package x.v1;\n");
        for i in 0..20_000 {
            s.push_str(&format!("message M{i} {{ 1: a int32; 2: b string; }}\n"));
        }
        let _ = check(&s, None);
    });
}

#[test]
fn huge_single_message_is_handled() {
    run_on_big_stack(|| {
        let mut s = String::from("package x.v1;\nmessage M {\n");
        for i in 1..=50_000 {
            s.push_str(&format!("  {i}: f{i} int32;\n"));
        }
        s.push_str("}\n");
        let _ = check(&s, None);
    });
}

#[test]
fn garbage_inputs_never_panic() {
    run_on_big_stack(|| {
        let cases: Vec<String> = vec![
            String::new(),
            "\0\0\0".into(),
            "package".into(),
            "message".into(),
            "message M {".into(),
            "package x.v1; message M { 1: a ".into(),
            "package x.v1; message M { 99999999999999999999: a int32; }".into(),
            "package x.v1; message M { -1: a int32; }".into(),
            "package x.v1; enum E { A = 99999999999999999999; }".into(),
            "package x.v1; message M { 1: a \"unterminated".into(),
            "/* unterminated comment".into(),
            "package x.v1; message M { 1: a M; } message M { 1: a int32; }".into(),
            "\u{feff}package x.v1;".into(),
            "package ".to_string() + &"a.".repeat(100_000) + "b;",
            "package x.v1; message ".to_string() + &"A".repeat(1_000_000) + " {}",
            "🦀".repeat(10_000),
            "{".repeat(200_000),
            "(".repeat(200_000),
            "<".repeat(200_000),
        ];
        for c in cases {
            let _ = check(&c, None);
        }
    });
}

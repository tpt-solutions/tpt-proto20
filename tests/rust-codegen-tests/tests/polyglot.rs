//! Cross-language interop: Go, Java and Python code generated from the same
//! schema must read and write exactly the bytes the Rust implementation does.
//!
//! For every sample (hand-built and randomly mutated, valid and malformed) each
//! language decodes the bytes and re-encodes the result; the output must be
//! byte-identical to Rust's own decode+encode, and a decode error in one
//! implementation must be a decode error in all. A language whose toolchain is
//! not installed is skipped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tpt20_codegen_backends::{backend, BackendOptions};
use tpt20_codegen_tests::generated::*;

const SCHEMA: &str = include_str!("../src/schema.tpt");

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg(if tool == "go" { "version" } else { "--version" })
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpt20-polyglot-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn generate(lang: &str, dir: &Path) {
    let ir = tpt20_compiler::compile(SCHEMA, Some("schema.tpt"))
        .map_err(|d| format!("{d:?}"))
        .unwrap()
        .ir;
    let files = backend(lang)
        .unwrap()
        .generate(&ir, &BackendOptions::default())
        .unwrap();
    for f in files {
        let path = dir.join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, f.contents).unwrap();
    }
}

// ---- Rust reference ---------------------------------------------------------

/// Rust's decode + re-encode; `None` is a decode error.
fn rust_roundtrip(ty: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    macro_rules! rt {
        ($t:ty) => {
            <$t>::decode(bytes).ok().map(|m| m.encode())
        };
    }
    match ty {
        "Outer" => rt!(Outer),
        "Address" => rt!(Address),
        "Outer_Child" => rt!(Outer_Child),
        "Diff" => rt!(Diff),
        "DiffInner" => rt!(DiffInner),
        "Tree" => rt!(Tree),
        "Expr" => rt!(Expr),
        other => panic!("unknown type {other}"),
    }
}

fn outer_sample() -> Outer {
    Outer {
        id: -5,
        name: "Ada é🦀".into(),
        email: Some("ada@example.com".into()),
        age: Some(42),
        username: "ada01".into(),
        tags: vec!["a".into(), "".into(), "zz".into()],
        scores: vec![1, -2, 300, i64::MIN],
        attrs: [("k".to_string(), "v".to_string()), ("a".into(), "b".into())]
            .into_iter()
            .collect(),
        counts: [(-7i64, "neg".to_string()), (3, "pos".into())]
            .into_iter()
            .collect(),
        contact: Some(OuterContact::EmailAddr("x@y.z".into())),
        status: Outer_Status::SUSPENDED,
        feature: Outer_Feature::Unknown(77),
        home: Some(Address {
            street: "1 Way".into(),
            city: Some(String::new()),
            ..Default::default()
        }),
        blob: vec![0xff, 0x00, 0x7f],
        ratio: -2.5,
        flags: vec![1, u32::MAX],
        inner: Some(Outer_Child {
            note: "n".into(),
            depth: 3,
            leaf: Some(Outer_Child_Leaf {
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
        ..Default::default()
    }
}

fn diff_sample() -> Diff {
    Diff {
        a: i32::MIN,
        b: i64::MAX,
        c: u32::MAX,
        d: u64::MAX,
        e: i32::MIN,
        f: i64::MIN,
        g: true,
        h: 0xdead_beef,
        i: u64::MAX - 1,
        j: i32::MIN,
        k: i64::MIN,
        l: -0.0,
        m: f64::MAX,
        s: "x".repeat(200),
        by: vec![0; 130],
        rp: vec![0, -1, i32::MAX, i32::MIN],
        rs: vec!["b".into(), "a".into()],
        rz: vec![i64::MIN, 0, 5],
        rd: vec![0.5, -1e300, f64::MIN_POSITIVE],
        m1: [("k".to_string(), i64::MIN), ("".into(), 0)]
            .into_iter()
            .collect(),
        nested: Some(DiffInner {
            x: "n".into(),
            y: -1,
            ..Default::default()
        }),
        opt: Some(0),
        mm: [("z".to_string(), DiffInner::default())]
            .into_iter()
            .collect(),
        rn: vec![
            DiffInner::default(),
            DiffInner {
                x: "q".into(),
                y: 9,
                ..Default::default()
            },
        ],
        pick: Some(DiffPick::Pb("p".into())),
        ..Default::default()
    }
}

fn tree_sample() -> Tree {
    let leaf = |v| Tree {
        value: v,
        ..Default::default()
    };
    Tree {
        value: 1,
        left: Some(Box::new(Tree {
            left: Some(Box::new(leaf(3))),
            ..leaf(2)
        })),
        right: Some(Box::new(leaf(-4))),
        children: vec![leaf(5), leaf(6)],
        by_name: [("x".to_string(), leaf(7))].into_iter().collect(),
        ..Default::default()
    }
}

fn expr_sample() -> Expr {
    Expr {
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
    }
}

/// Deterministic xorshift.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// `(type, bytes)` cases: valid samples, truncations, and random mutations.
fn cases() -> Vec<(&'static str, Vec<u8>)> {
    let valid: Vec<(&'static str, Vec<u8>)> = vec![
        ("Outer", Outer::default().encode()),
        ("Outer", outer_sample().encode()),
        ("Outer_Child", outer_sample().inner.unwrap().encode()),
        ("Address", outer_sample().home.unwrap().encode()),
        ("Diff", Diff::default().encode()),
        ("Diff", diff_sample().encode()),
        (
            "DiffInner",
            DiffInner {
                x: "é".into(),
                y: -3,
                ..Default::default()
            }
            .encode(),
        ),
        ("Tree", tree_sample().encode()),
        ("Expr", expr_sample().encode()),
    ];
    let mut out = valid.clone();
    // Well-formed variations: concatenating two encodings merges them (singular
    // fields: last wins, repeated: accumulate, oneofs: last wins), and unknown
    // fields of every wire class are carried through.
    for (ty, a) in &valid {
        for (ty2, b) in &valid {
            if ty == ty2 {
                out.push((ty, [a.as_slice(), b.as_slice()].concat()));
            }
        }
        for unknown in [
            &[0xf8u8, 0x06, 0x2a][..],             // field 111 varint 42
            &[0xf9, 0x06, 1, 2, 3, 4],             // field 111 fixed32
            &[0xfa, 0x06, 1, 2, 3, 4, 5, 6, 7, 8], // field 111 fixed64
            &[0xfb, 0x06, 3, b'a', b'b', b'c'],    // field 111 len
        ] {
            out.push((ty, [a.as_slice(), unknown].concat()));
            out.push((ty, [unknown, a.as_slice()].concat()));
        }
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for (ty, bytes) in &valid {
        // Every truncation of each sample.
        for n in 0..bytes.len().min(400) {
            out.push((ty, bytes[..n].to_vec()));
        }
        // Random mutations: flip, insert, delete, duplicate a span.
        for _ in 0..120 {
            let mut b = bytes.clone();
            if b.is_empty() {
                continue;
            }
            for _ in 0..(1 + rng.next() % 3) {
                let i = (rng.next() as usize) % b.len().max(1);
                match rng.next() % 4 {
                    0 => b[i] ^= 1 << (rng.next() % 8),
                    1 => b.insert(i, rng.next() as u8),
                    2 if b.len() > 1 => {
                        b.remove(i);
                    }
                    _ => {
                        let end = (i + 1 + (rng.next() as usize) % 8).min(b.len());
                        let span = b[i..end].to_vec();
                        b.extend(span);
                    }
                }
                if b.is_empty() {
                    b.push(0);
                }
            }
            out.push((ty, b));
        }
    }
    out
}

fn hex(b: &[u8]) -> String {
    if b.is_empty() {
        "-".into()
    } else {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}

/// Runs `cmd`, feeding the cases on stdin; one output line per case.
fn run(mut cmd: Command, cases: &[(&str, Vec<u8>)]) -> Vec<String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn driver");
    let mut stdin = child.stdin.take().unwrap();
    let input: String = cases
        .iter()
        .map(|(t, b)| format!("{t} {}\n", hex(b)))
        .collect();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        out.status.success(),
        "driver failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .last()
            .unwrap_or(""),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

fn compare(lang: &str, cases: &[(&'static str, Vec<u8>)], got: Vec<String>) {
    assert_eq!(got.len(), cases.len(), "{lang}: one line per case");
    // The corpus must exercise both outcomes, or agreement would be vacuous.
    let accepted = got.iter().filter(|l| *l != "ERR").count();
    assert!(accepted > 200, "{lang}: only {accepted} cases decoded");
    assert!(got.len() - accepted > 500, "{lang}: too few rejected cases");
    let mut mismatches: BTreeMap<String, usize> = BTreeMap::new();
    let mut first: Vec<String> = Vec::new();
    for ((ty, bytes), line) in cases.iter().zip(&got) {
        let want = match rust_roundtrip(ty, bytes) {
            Some(b) => hex(&b),
            None => "ERR".to_string(),
        };
        if *line != want {
            *mismatches.entry((*ty).to_string()).or_default() += 1;
            if first.len() < 8 {
                first.push(format!(
                    "{ty} in={}\n   rust={want}\n   {lang}={line}",
                    hex(bytes)
                ));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{lang} disagrees with Rust on {} of {} cases {mismatches:?}\n{}",
        mismatches.values().sum::<usize>(),
        cases.len(),
        first.join("\n")
    );
}

const PY_DRIVER: &str = r#"
import sys
import codegen_test_v1 as s
import tpt20_runtime as rt
for line in sys.stdin:
    t, h = line.split()
    data = b"" if h == "-" else bytes.fromhex(h)
    try:
        out = getattr(s, t).decode(data).encode()
        print(out.hex() or "-")
    except rt.DecodeError:
        print("ERR")
"#;

#[test]
fn python_agrees_with_rust() {
    if !have("python3") {
        eprintln!("python3 not installed; skipping");
        return;
    }
    let dir = scratch("py");
    generate("python", &dir);
    std::fs::write(dir.join("driver.py"), PY_DRIVER).unwrap();
    let cases = cases();
    let mut cmd = Command::new("python3");
    cmd.arg("driver.py").current_dir(&dir);
    compare("python", &cases, run(cmd, &cases));
}

const GO_DRIVER: &str = r#"
package main

import (
	"bufio"
	"encoding/hex"
	"fmt"
	"os"
	"strings"

	s "codegen_test_v1"
)

func rt[T interface{ Encode() []byte }](data []byte, dec func([]byte) (T, error)) ([]byte, error) {
	m, err := dec(data)
	if err != nil {
		return nil, err
	}
	return m.Encode(), nil
}

func main() {
	sc := bufio.NewScanner(os.Stdin)
	sc.Buffer(make([]byte, 1<<20), 1<<26)
	for sc.Scan() {
		parts := strings.Fields(sc.Text())
		var data []byte
		if parts[1] != "-" {
			data, _ = hex.DecodeString(parts[1])
		}
		var out []byte
		var err error
		switch parts[0] {
		case "Outer":
			out, err = rt(data, s.DecodeOuter)
		case "Address":
			out, err = rt(data, s.DecodeAddress)
		case "Outer_Child":
			out, err = rt(data, s.DecodeOuter_Child)
		case "Diff":
			out, err = rt(data, s.DecodeDiff)
		case "DiffInner":
			out, err = rt(data, s.DecodeDiffInner)
		case "Tree":
			out, err = rt(data, s.DecodeTree)
		case "Expr":
			out, err = rt(data, s.DecodeExpr)
		default:
			panic("unknown type " + parts[0])
		}
		if err != nil {
			if !s.IsTpt20DecodeError(err) {
				panic(err)
			}
			fmt.Println("ERR")
		} else if len(out) == 0 {
			fmt.Println("-")
		} else {
			fmt.Println(hex.EncodeToString(out))
		}
	}
}
"#;

#[test]
fn go_agrees_with_rust() {
    if !have("go") {
        eprintln!("go not installed; skipping");
        return;
    }
    let dir = scratch("go");
    generate("go", &dir);
    std::fs::create_dir_all(dir.join("driver")).unwrap();
    std::fs::write(dir.join("driver/main.go"), GO_DRIVER).unwrap();
    let cases = cases();
    let mut cmd = Command::new("go");
    cmd.args(["run", "./driver"])
        .current_dir(&dir)
        .env("GOFLAGS", "-mod=mod")
        .env("GOCACHE", std::env::temp_dir().join("tpt20-gocache"));
    compare("go", &cases, run(cmd, &cases));
}

const JAVA_DRIVER: &str = r#"
import codegen_test_v1.*;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class Driver {
    static byte[] unhex(String h) {
        if (h.equals("-")) return new byte[0];
        byte[] out = new byte[h.length() / 2];
        for (int i = 0; i < out.length; i++) {
            out[i] = (byte) Integer.parseInt(h.substring(2 * i, 2 * i + 2), 16);
        }
        return out;
    }

    static String hex(byte[] b) {
        if (b.length == 0) return "-";
        StringBuilder sb = new StringBuilder();
        for (byte x : b) sb.append(String.format("%02x", x & 0xff));
        return sb.toString();
    }

    static byte[] roundtrip(String type, byte[] data) {
        switch (type) {
            case "Outer": return Outer.decode(data).encode();
            case "Address": return Address.decode(data).encode();
            case "Outer_Child": return Outer_Child.decode(data).encode();
            case "Diff": return Diff.decode(data).encode();
            case "DiffInner": return DiffInner.decode(data).encode();
            case "Tree": return Tree.decode(data).encode();
            case "Expr": return Expr.decode(data).encode();
            default: throw new IllegalStateException("unknown type " + type);
        }
    }

    public static void main(String[] args) throws Exception {
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        PrintStream out = new PrintStream(new BufferedOutputStream(System.out), false, "UTF-8");
        String line;
        while ((line = in.readLine()) != null) {
            String[] parts = line.trim().split(" ");
            try {
                out.println(hex(roundtrip(parts[0], unhex(parts[1]))));
            } catch (Tpt20Runtime.DecodeException e) {
                out.println("ERR");
            }
        }
        out.flush();
    }
}
"#;

#[test]
fn java_agrees_with_rust() {
    if !have("javac") || !have("java") {
        eprintln!("java not installed; skipping");
        return;
    }
    let dir = scratch("java");
    generate("java", &dir);
    std::fs::write(dir.join("Driver.java"), JAVA_DRIVER).unwrap();
    let sources: Vec<String> = std::fs::read_dir(dir.join("codegen_test_v1"))
        .unwrap()
        .map(|e| {
            format!(
                "codegen_test_v1/{}",
                e.unwrap().file_name().to_string_lossy()
            )
        })
        .chain(["Driver.java".to_string()])
        .collect();
    let compile = Command::new("javac")
        .args(["-d", "out", "-Xlint:all"])
        .args(&sources)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let cases = cases();
    let mut cmd = Command::new("java");
    cmd.args(["-cp", "out", "Driver"]).current_dir(&dir);
    compare("java", &cases, run(cmd, &cases));
}

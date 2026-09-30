//! End-to-end tests for the `tpt20` binary against a live in-process HTTP/2
//! server from `tpt20-transport`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use tpt20_transport::http2::{Http2Server, IncomingHttp2Call};
use tpt20_transport::{Endpoint, Metadata, TlsConfig, TransportError};

type Boxed = futures::future::BoxFuture<'static, Result<(), TransportError>>;

const SCHEMA: &str = r#"
package cli.v1;
message Leaf { 1: v int64; }
message Item {
  1: id int64;
  2: name string;
  3: tags repeated string;
  4: leaf Leaf;
}
"#;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn tmp(name: &str, contents: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt20-cli-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn tpt20(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tpt20"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn handler(mut call: IncomingHttp2Call) -> Boxed {
    Box::pin(async move {
        match call.method.as_str() {
            "tpt20.health.v1.Health/Check" => {
                // Field 1 is the requested service name.
                let svc = tpt20_core::RawMessage::decode(
                    &call.request,
                    &tpt20_core::DecoderLimits::default(),
                    tpt20_core::UnknownFieldPolicy::Preserve,
                )
                .unwrap();
                let name = svc
                    .fields
                    .iter()
                    .find_map(|f| match &f.value {
                        tpt20_core::Value::Len(b) => Some(String::from_utf8_lossy(b).into_owned()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let status = match name.as_str() {
                    "" | "ok" => 1,
                    "down" => 2,
                    _ => 3,
                };
                let mut resp = tpt20_core::RawMessage::new();
                resp.push(tpt20_core::Field::new(
                    1,
                    tpt20_core::WireClass::Varint,
                    tpt20_core::Value::Varint(status),
                ));
                call.send_message(resp.encode().unwrap()).await?;
                // The RPC layer requires a final status.
                let mut md = Metadata::new();
                md.insert("grpc-status", "0");
                call.send_trailers(md).await?;
            }
            "Slow" => {
                tokio::time::sleep(Duration::from_secs(3)).await;
                call.send_message(vec![]).await?;
            }
            "Count" => {
                let mut n = 1;
                while call.recv_message().await.is_some() {
                    n += 1;
                }
                let mut resp = tpt20_core::RawMessage::new();
                resp.push(tpt20_core::Field::new(
                    1,
                    tpt20_core::WireClass::Varint,
                    tpt20_core::Value::Varint(n),
                ));
                call.send_message(resp.encode().unwrap()).await?;
            }
            _ => {
                // Echo request back; report the `x-k` metadata in a trailer.
                call.send_message(call.request.clone()).await?;
                let mut md = Metadata::new();
                md.insert("x-status", "ok");
                if let Some(v) = call.metadata.get("x-k") {
                    md.insert("x-seen-k", v.join(","));
                }
                call.send_trailers(md).await?;
            }
        }
        Ok(())
    })
}

/// Starts a server on its own runtime thread and returns `host:port`.
fn start_server(endpoint: Endpoint, host: &str) -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap().port()).unwrap();
            let _ = Http2Server::new(endpoint)
                .serve_listener(listener, handler, std::future::pending())
                .await;
        });
    });
    format!("{host}:{}", rx.recv().unwrap())
}

#[test]
fn text_binary_roundtrip_with_schema() {
    let schema = tmp("s.tpt", SCHEMA.as_bytes());
    let text = tmp(
        "in.txt",
        b"id: 7\nname: \"Ada\"\ntags: \"a\"\ntags: \"b\"\nleaf { v: -1 }\n",
    );
    let bin = tmp("out.bin", b"");
    let s = schema.to_str().unwrap();
    let o = tpt20(&[
        "text-to-binary",
        "-s",
        s,
        "-m",
        "Item",
        "-i",
        text.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(!fs::read(&bin).unwrap().is_empty());

    let o = tpt20(&[
        "binary-to-text",
        "-s",
        s,
        "-m",
        "Item",
        "-i",
        bin.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "id: 7\nname: \"Ada\"\ntags: \"a\"\ntags: \"b\"\nleaf {\n  v: -1\n}\n"
    );

    // decode --schema is the same schema-aware view.
    let o = tpt20(&["decode", "-s", s, "-m", "Item", "-i", bin.to_str().unwrap()]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("name: \"Ada\""));

    // Bad text is an error, not silently skipped.
    let bad = tmp("bad.txt", b"nope: 1\n");
    let o = tpt20(&[
        "text-to-binary",
        "-s",
        s,
        "-m",
        "Item",
        "-i",
        bad.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("unknown field"), "{}", stderr(&o));
}

#[test]
fn call_unary_prints_response_and_trailers() {
    let addr = start_server(Endpoint::new("x"), "127.0.0.1");
    let input = tmp("req.json", br#"{"1": 42, "2": "hi"}"#);
    let o = tpt20(&[
        "call",
        &format!("http://{addr}"),
        "pkg.Echo",
        "-i",
        input.to_str().unwrap(),
        "-m",
        "x-k=v1",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("# message 1"), "{out}");
    assert!(out.contains("1: 42"), "{out}");
    assert!(out.contains("2: \"hi\""), "{out}");
    assert!(out.contains("x-status: ok"), "{out}");
    assert!(out.contains("x-seen-k: v1"), "{out}");
}

#[test]
fn call_with_schema_text_in_and_out() {
    let addr = start_server(Endpoint::new("x"), "127.0.0.1");
    let schema = tmp("s.tpt", SCHEMA.as_bytes());
    let text = tmp("req.txt", b"id: 9\nname: \"Bo\"\n");
    let o = tpt20(&[
        "call",
        &addr,
        "pkg.Echo",
        "--schema",
        schema.to_str().unwrap(),
        "--request-type",
        "Item",
        "--response-type",
        "Item",
        "--text-input",
        text.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(
        stdout(&o).contains("id: 9\nname: \"Bo\"\n"),
        "{}",
        stdout(&o)
    );
}

#[test]
fn call_client_streaming_sends_every_array_element() {
    let addr = start_server(Endpoint::new("x"), "127.0.0.1");
    let input = tmp("reqs.json", br#"[{"1": 1}, {"1": 2}, {"1": 3}, {"1": 4}]"#);
    let o = tpt20(&[
        "call",
        &addr,
        "Count",
        "-s",
        "client",
        "-i",
        input.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("1: 4"), "{}", stdout(&o));

    // Unary calls reject more than one message.
    let o = tpt20(&["call", &addr, "Count", "-i", input.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
}

#[test]
fn call_enforces_deadline_supports_compression_and_reports_errors() {
    let addr = start_server(Endpoint::new("x"), "127.0.0.1");
    let input = tmp("req.json", br#"{"1": 1}"#);
    let inp = input.to_str().unwrap();
    let o = tpt20(&["call", &addr, "Slow", "-i", inp, "-d", "200"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("deadline"), "{}", stderr(&o));

    // Compression is negotiated with the server, which here has none
    // configured: the call still works (request compressed, reply plain).
    let big = tmp(
        "big.json",
        format!("{{\"1\": \"{}\"}}", "x".repeat(5000)).as_bytes(),
    );
    for alg in ["gzip", "deflate"] {
        let o = tpt20(&[
            "call",
            &addr,
            "X",
            "-i",
            big.to_str().unwrap(),
            "--compression",
            alg,
        ]);
        assert!(o.status.success(), "{alg}: {}", stderr(&o));
        assert!(stdout(&o).contains("# message 1"), "{}", stdout(&o));
    }
    let o = tpt20(&["call", &addr, "X", "-i", inp, "--compression", "zstd"]);
    assert_eq!(
        o.status.code(),
        Some(2),
        "unknown algorithms are usage errors"
    );

    // Nothing listening: a real connection error, exit 1.
    let o = tpt20(&["call", "127.0.0.1:1", "X", "-i", inp]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("transport error"), "{}", stderr(&o));
}

#[test]
fn health_reports_status_through_exit_code() {
    let addr = start_server(Endpoint::new("x"), "127.0.0.1");
    let o = tpt20(&["health", &addr]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("SERVING"));

    let o = tpt20(&["health", &addr, "--service", "down"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("NOT_SERVING"), "{}", stdout(&o));

    let o = tpt20(&["health", &addr, "--service", "what"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("SERVICE_UNKNOWN"));
}

#[test]
fn health_and_call_over_tls_with_custom_ca() {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let mut tls = TlsConfig::http2();
    tls.cert_pem = Some(cert.cert.pem().into_bytes());
    tls.key_pem = Some(cert.key_pair.serialize_pem().into_bytes());
    let addr = start_server(Endpoint::new("x").with_tls(tls), "localhost");
    let ca = tmp("ca.pem", cert.cert.pem().as_bytes());

    let o = tpt20(&[
        "health",
        &format!("https://{addr}"),
        "--tls-cert",
        ca.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("SERVING"));

    // https without a CA to trust is a usage error.
    let o = tpt20(&["health", &format!("https://{addr}")]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
}

#[test]
fn registry_publish_list_get_roundtrip_and_immutability() {
    let dir = tmp("marker", b"");
    let reg = dir.parent().unwrap().join("registry");
    let reg = reg.to_str().unwrap();
    let v1 = tmp("a.tpt", b"package reg.v1;\nmessage A { 1: id int64; }\n");
    let v1b = tmp(
        "b.tpt",
        b"package reg.v1;\nmessage A { 1: id int64; 2: name string; }\n",
    );

    let o = tpt20(&[
        "registry",
        "publish",
        v1.to_str().unwrap(),
        "-r",
        reg,
        "-v",
        "1.0.0",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));

    // Re-publishing identical content is a no-op.
    let o = tpt20(&[
        "registry",
        "publish",
        v1.to_str().unwrap(),
        "-r",
        reg,
        "-v",
        "1.0.0",
    ]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("already published"));

    // Different content under the same label is refused...
    let o = tpt20(&[
        "registry",
        "publish",
        v1b.to_str().unwrap(),
        "-r",
        reg,
        "-v",
        "1.0.0",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("immutable"), "{}", stderr(&o));
    // ...unless forced; a new label is always fine.
    let o = tpt20(&[
        "registry",
        "publish",
        v1b.to_str().unwrap(),
        "-r",
        reg,
        "-v",
        "1.1.0",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));

    let o = tpt20(&["registry", "list", "-r", reg]);
    let out = stdout(&o);
    assert!(out.contains("1.0.0") && out.contains("1.1.0"), "{out}");
    assert!(!out.contains("\tnow"), "{out}");

    // get by label → valid descriptor JSON; binary format also works.
    let o = tpt20(&["registry", "get", "1.1.0", "-r", reg]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("\"name\""), "{}", stdout(&o));
    let o = tpt20(&["registry", "get", "1.0.0", "-r", reg, "-f", "binary"]);
    assert!(o.status.success() && !o.stdout.is_empty());

    // get by fingerprint prefix (from `list`'s 16-char column)
    let listing = stdout(&tpt20(&["registry", "list", "-r", reg]));
    let fp = listing
        .lines()
        .find(|l| l.starts_with("1.0.0"))
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap()
        .to_string();
    let o = tpt20(&["registry", "get", &fp, "-r", reg]);
    assert!(o.status.success(), "{}", stderr(&o));

    // unknown versions and unsafe labels are errors
    let o = tpt20(&["registry", "get", "9.9.9", "-r", reg]);
    assert_eq!(o.status.code(), Some(1));
    let o = tpt20(&[
        "registry",
        "publish",
        v1.to_str().unwrap(),
        "-r",
        reg,
        "-v",
        "../escape",
    ]);
    assert_eq!(o.status.code(), Some(2));

    // tampering with a stored descriptor is detected on fetch
    let stored = std::path::Path::new(reg)
        .join("1.0.0")
        .join("descriptor.json");
    let original = fs::read_to_string(&stored).unwrap();
    let tampered = original
        .replace("\"name\": \"id\"", "\"name\": \"idx\"")
        .replace("\"name\":\"id\"", "\"name\":\"idx\"");
    assert_ne!(tampered, original, "tamper must change the descriptor");
    fs::write(&stored, tampered).unwrap();
    let o = tpt20(&["registry", "get", "1.0.0", "-r", reg]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("fingerprint"), "{}", stderr(&o));
}

/// Serves a `tpt20_rpc::Server` over HTTP/2 on its own runtime thread.
fn start_rpc_server(server: std::sync::Arc<tpt20_rpc::Server>) -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap().port()).unwrap();
            let _ = Http2Server::new(Endpoint::new("x"))
                .serve_listener(
                    listener,
                    move |call| {
                        let server = server.clone();
                        Box::pin(async move {
                            server.handle_call(call).await;
                            Ok(())
                        })
                    },
                    std::future::pending(),
                )
                .await;
        });
    });
    format!("127.0.0.1:{}", rx.recv().unwrap())
}

#[test]
fn builtin_health_and_reflection_services_work_with_the_cli() {
    use tpt20_rpc::health::{HealthService, ServingStatus};
    use tpt20_rpc::reflection::ReflectionService;

    let schema = "package rfl.v1;\nmessage M { 1: id int64; }\nservice S { Do(M) returns (M); }\n";
    let compiled = tpt20_compiler::compile(schema, None).unwrap();
    let mut descriptor = tpt20_descriptor::Descriptor::new(compiled.ir);
    let fingerprint = descriptor.compute_fingerprint();
    let bytes = descriptor.to_binary().unwrap();

    let (health, reporter) = HealthService::new();
    reporter.set_status("rfl.v1.S", ServingStatus::Serving);
    reporter.set_status("rfl.v1.Down", ServingStatus::NotServing);
    let reflection =
        ReflectionService::new().register("rfl.v1", &bytes, fingerprint, &["rfl.v1.S"]);
    let addr = start_rpc_server(std::sync::Arc::new(
        tpt20_rpc::Server::new()
            .add_service(health)
            .add_service(reflection),
    ));

    // health
    assert!(tpt20(&["health", &addr]).status.success());
    let o = tpt20(&["health", &addr, "-s", "rfl.v1.S"]);
    assert!(o.status.success() && stdout(&o).contains("SERVING"));
    let o = tpt20(&["health", &addr, "-s", "rfl.v1.Down"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("NOT_SERVING"), "{}", stdout(&o));
    let o = tpt20(&["health", &addr, "-s", "nope"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("SERVICE_UNKNOWN"));

    // reflection: service list, descriptor as JSON and binary
    let o = tpt20(&["reflect-remote", &addr]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(stdout(&o).trim(), "rfl.v1.S");
    let o = tpt20(&["reflect-remote", &addr, "-d"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("\"M\""), "{}", stdout(&o));
    let o = tpt20(&[
        "reflect-remote",
        &addr,
        "-d",
        "-f",
        "binary",
        "-p",
        "rfl.v1",
    ]);
    assert!(o.status.success());
    assert_eq!(o.stdout, bytes);
    let o = tpt20(&["reflect-remote", &addr, "-d", "-p", "missing.v1"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("NOT_FOUND"), "{}", stderr(&o));
}

fn vectors_dir() -> String {
    format!("{}/../../conformance/vectors", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn conformance_runs_the_shipped_vectors() {
    let o = tpt20(&["conformance", "-d", &vectors_dir()]);
    assert!(o.status.success(), "{}{}", stdout(&o), stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("PASS wire_decode/varint_150"), "{out}");
    assert!(
        out.contains("PASS text_format/nested_message_and_map"),
        "{out}"
    );
    assert!(out.contains(" 0 failed"), "{out}");

    // Filtering by suite or case name.
    let o = tpt20(&[
        "conformance",
        "-d",
        &vectors_dir(),
        "-t",
        "canonical_encoding",
    ]);
    assert!(o.status.success());
    assert!(!stdout(&o).contains("wire_decode"), "{}", stdout(&o));
    let o = tpt20(&["conformance", "-d", &vectors_dir(), "-t", "no_such_case"]);
    assert_eq!(
        o.status.code(),
        Some(2),
        "matching nothing is a usage error"
    );
}

#[test]
fn conformance_reports_failing_cases_with_exit_code_1() {
    let bad = tmp(
        "vectors.json",
        br#"{"suite":"bad","cases":[
            {"name":"wrong_fields","kind":"decode","hex":"0801","expect":{"fields":[{"id":1,"class":"varint","value":"2"}]}},
            {"name":"wrong_error","kind":"decode","hex":"08","expect_error":"VarintOverflow"},
            {"name":"not_canonical","kind":"canonical","fields":[{"id":2,"class":"varint","value":"1"},{"id":1,"class":"varint","value":"1"}],"hex":"10010801"},
            {"name":"good","kind":"roundtrip","hex":"0801"}
        ]}"#,
    );
    let dir = bad.parent().unwrap().to_str().unwrap().to_string();
    let o = tpt20(&["conformance", "-d", &dir]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    for case in ["wrong_fields", "wrong_error", "not_canonical"] {
        assert!(err.contains(&format!("FAIL bad/{case}")), "{err}");
    }
    assert!(stdout(&o).contains("PASS bad/good"));
    assert!(stdout(&o).contains("1 passed, 3 failed"), "{}", stdout(&o));
}

#[test]
fn gen_python_go_java_write_code_and_runtime() {
    let dir = std::env::temp_dir().join(format!("tpt20-cli-gen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let schema = dir.join("demo.tpt");
    std::fs::write(
        &schema,
        "package demo.v1;\nmessage Ping { 1: text string; 2: n int32; }\n",
    )
    .unwrap();
    for (lang, expected) in [
        ("python", vec!["demo_v1.py", "tpt20_runtime.py"]),
        ("go", vec!["go.mod", "demo_v1.go", "tpt20_runtime.go"]),
        (
            "java",
            vec!["demo_v1/Ping.java", "demo_v1/Tpt20Runtime.java"],
        ),
    ] {
        let out = dir.join(lang);
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_tpt20"))
            .args(["gen", lang, "--in"])
            .arg(&schema)
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap();
        assert!(status.status.success(), "{lang}: {status:?}");
        for f in expected {
            assert!(out.join(f).is_file(), "{lang}: missing {f}");
        }
    }
}

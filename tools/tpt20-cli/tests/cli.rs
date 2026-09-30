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
fn call_enforces_deadline_and_rejects_unsupported_options() {
    let addr = start_server(Endpoint::new("x"), "127.0.0.1");
    let input = tmp("req.json", br#"{"1": 1}"#);
    let inp = input.to_str().unwrap();
    let o = tpt20(&["call", &addr, "Slow", "-i", inp, "-d", "200"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("deadline"), "{}", stderr(&o));

    let o = tpt20(&["call", &addr, "X", "-i", inp, "--compression", "gzip"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("compression"), "{}", stderr(&o));

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

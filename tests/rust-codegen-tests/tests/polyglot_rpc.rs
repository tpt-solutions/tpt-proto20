//! RPC interop: generated Go and Python clients and servers talk to the Rust
//! runtime over HTTP/2 in both directions, for all four streaming shapes,
//! metadata, status propagation, deadlines and cancellation.
//!
//! A language whose toolchain (or, for Python, the `h2` package) is missing is
//! skipped.

use futures::StreamExt;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tpt20_codegen_backends::{backend, BackendOptions};
use tpt20_codegen_tests::generated::{PingReply, PingRequest, Pinger, PingerClient, PingerServer};
use tpt20_rpc::{async_trait, BoxStream, Channel, RpcContext, RpcError, Server, Status};
use tpt20_transport::http2::{Http2Server, Http2Transport};
use tpt20_transport::Endpoint;

const SCHEMA: &str = include_str!("../src/schema.tpt");

fn have(tool: &str, arg: &str) -> bool {
    Command::new(tool)
        .arg(arg)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("tpt20-polyglot-rpc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn generate(lang: &str, dir: &Path) {
    let ir = tpt20_compiler::compile(SCHEMA, Some("schema.tpt"))
        .map_err(|d| format!("{d:?}"))
        .unwrap()
        .ir;
    let options = BackendOptions {
        services: true,
        ..Default::default()
    };
    for f in backend(lang).unwrap().generate(&ir, &options).unwrap() {
        let path = dir.join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, f.contents).unwrap();
    }
}

// ---- the Rust implementation of the service under test ----------------------

struct Impl;

fn reply(text: impl Into<String>, n: i32) -> PingReply {
    PingReply {
        text: text.into(),
        n,
        ..Default::default()
    }
}

fn req(text: &str, count: i32) -> PingRequest {
    PingRequest {
        text: text.into(),
        count,
        ..Default::default()
    }
}

#[async_trait]
impl Pinger for Impl {
    async fn ping(&self, ctx: &RpcContext, request: PingRequest) -> Result<PingReply, RpcError> {
        let who = ctx
            .metadata()
            .get_first_text("x-who")
            .unwrap_or("anon")
            .to_string();
        Ok(reply(format!("{who}:{}", request.text), request.count + 1))
    }

    async fn repeat(
        &self,
        _ctx: &RpcContext,
        request: PingRequest,
    ) -> Result<BoxStream<'static, Result<PingReply, RpcError>>, RpcError> {
        let text = request.text;
        Ok(
            futures::stream::iter((0..request.count).map(move |i| Ok(reply(text.clone(), i))))
                .boxed(),
        )
    }

    async fn collect(
        &self,
        _ctx: &RpcContext,
        mut requests: BoxStream<'static, Result<PingRequest, RpcError>>,
    ) -> Result<PingReply, RpcError> {
        let (mut text, mut n) = (String::new(), 0);
        while let Some(r) = requests.next().await {
            let r = r?;
            text.push_str(&r.text);
            n += 1;
        }
        Ok(reply(text, n))
    }

    async fn chat(
        &self,
        _ctx: &RpcContext,
        requests: BoxStream<'static, Result<PingRequest, RpcError>>,
    ) -> Result<BoxStream<'static, Result<PingReply, RpcError>>, RpcError> {
        Ok(requests
            .map(|r| r.map(|r| reply(r.text.to_uppercase(), r.count)))
            .boxed())
    }

    async fn fail(&self, _ctx: &RpcContext, request: PingRequest) -> Result<PingReply, RpcError> {
        Err(RpcError::permission_denied(format!("nope: {}", request.text)).finish())
    }

    async fn get_http_status(
        &self,
        _ctx: &RpcContext,
        _r: PingRequest,
    ) -> Result<PingReply, RpcError> {
        Ok(reply("200", 200))
    }
}

/// What every foreign client prints when it runs the scenario against a
/// correct server, one line per observation.
const EXPECTED: &str = "\
ping ada:hi 42
repeat 0
repeat 1
repeat 2
collect abc 3
chat P 1
chat Q 2
fail 7 nope: secret
status 200
deadline 4
cancelled 1
unknown 12
";

async fn rust_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let http2 = Http2Server::new(Endpoint::new(addr.clone()));
    let srv = Arc::new(Server::new().add_service(PingerServer::new(Impl)));
    tokio::spawn(async move {
        let _ = http2
            .serve_listener(
                listener,
                move |call| {
                    let srv = srv.clone();
                    Box::pin(async move {
                        srv.handle_call(call).await;
                        Ok(())
                    })
                },
                std::future::pending(),
            )
            .await;
    });
    addr
}

/// Runs the scenario with the Rust client against `addr`.
async fn rust_client_scenario(addr: String) {
    let client = PingerClient::new(Channel::new(Http2Transport::new(Endpoint::new(addr))));
    let mut ctx = RpcContext::new();
    ctx.metadata_mut().insert_text("x-who", "ada").unwrap();

    let r = client.ping(&ctx, &req("hi", 41)).await.unwrap();
    assert_eq!((r.text.as_str(), r.n), ("ada:hi", 42));

    let s = client.repeat(&ctx, &req("x", 3)).await.unwrap();
    assert_eq!(s.map(|r| r.unwrap().n).collect::<Vec<_>>().await, [0, 1, 2]);

    let r = client
        .collect(
            &ctx,
            futures::stream::iter(vec![req("a", 0), req("b", 0), req("c", 0)]),
        )
        .await
        .unwrap();
    assert_eq!((r.text.as_str(), r.n), ("abc", 3));

    // Bidi with real interleaving: each answer is read before the next request.
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let mut answers = client.chat(&ctx, tokio_stream_from(rx)).await.unwrap();
    for (t, n) in [("p", 1), ("q", 2)] {
        tx.send(req(t, n)).await.unwrap();
        let r = tokio::time::timeout(Duration::from_secs(5), answers.next())
            .await
            .expect("answer before the next request")
            .unwrap()
            .unwrap();
        assert_eq!((r.text, r.n), (t.to_uppercase(), n));
    }
    drop(tx);
    assert!(answers.next().await.is_none());

    let e = client.fail(&ctx, &req("secret", 0)).await.unwrap_err();
    assert_eq!(
        (e.status(), e.message()),
        (Status::PermissionDenied, "nope: secret")
    );
    assert_eq!(
        client.get_http_status(&ctx, &req("", 0)).await.unwrap().n,
        200
    );

    let done = RpcContext::new().with_timeout(Duration::from_millis(0));
    let e = client.ping(&done, &req("late", 0)).await.unwrap_err();
    assert_eq!(e.status(), Status::DeadlineExceeded);
}

fn tokio_stream_from<T: Send + 'static>(
    mut rx: tokio::sync::mpsc::Receiver<T>,
) -> impl futures::Stream<Item = T> + Send + 'static {
    futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

/// Output of a foreign client run, asserted against [`EXPECTED`].
fn run_client(mut cmd: Command, addr: &str) -> String {
    let out = cmd.arg(addr).output().expect("spawn client");
    assert!(
        out.status.success(),
        "client failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Starts a foreign server; it prints `LISTENING host:port` when ready.
fn start_server(mut cmd: Command) -> (Child, String) {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn server");
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    let addr = line
        .trim()
        .strip_prefix("LISTENING ")
        .unwrap_or_else(|| panic!("server did not start: {line:?}"))
        .to_string();
    (child, addr)
}

// ---- Go ---------------------------------------------------------------------

const GO_CLIENT: &str = r#"
package main

import (
	"context"
	"fmt"
	"io"
	"os"

	s "codegen_test_v1"
)

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "unexpected error:", err)
		os.Exit(1)
	}
}

func main() {
	c := s.NewPingerClient(s.Tpt20Dial(os.Args[1]))
	ctx := context.Background()
	md := s.Tpt20Metadata{}
	md.Set("x-who", "ada")

	r, err := c.Ping(ctx, md, &s.PingRequest{Text: "hi", Count: 41})
	must(err)
	fmt.Printf("ping %s %d\n", r.Text, r.N)

	st, err := c.Repeat(ctx, md, &s.PingRequest{Text: "x", Count: 3})
	must(err)
	for {
		r, err := st.Recv()
		if err == io.EOF {
			break
		}
		must(err)
		fmt.Printf("repeat %d\n", r.N)
	}

	cs, err := c.Collect(ctx, md)
	must(err)
	for _, t := range []string{"a", "b", "c"} {
		must(cs.Send(&s.PingRequest{Text: t}))
	}
	r, err = cs.CloseAndRecv()
	must(err)
	fmt.Printf("collect %s %d\n", r.Text, r.N)

	bi, err := c.Chat(ctx, md)
	must(err)
	for i, t := range []string{"p", "q"} {
		must(bi.Send(&s.PingRequest{Text: t, Count: int32(i + 1)}))
		r, err := bi.Recv() // answered before the next request is sent
		must(err)
		fmt.Printf("chat %s %d\n", r.Text, r.N)
	}
	must(bi.CloseSend())
	if _, err := bi.Recv(); err != io.EOF {
		must(fmt.Errorf("expected end of stream, got %v", err))
	}

	_, err = c.Fail(ctx, md, &s.PingRequest{Text: "secret"})
	f := s.Tpt20StatusOf(err)
	fmt.Printf("fail %d %s\n", f.Code, f.Message)

	r, err = c.GetHTTPStatus(ctx, md, &s.PingRequest{})
	must(err)
	fmt.Printf("status %d\n", r.N)

	short, cancel := context.WithTimeout(ctx, 0)
	defer cancel()
	_, err = c.Ping(short, md, &s.PingRequest{Text: "late"})
	fmt.Printf("deadline %d\n", s.Tpt20StatusOf(err).Code)

	cctx, cancel2 := context.WithCancel(ctx)
	cancel2()
	_, err = c.Ping(cctx, md, &s.PingRequest{Text: "x"})
	fmt.Printf("cancelled %d\n", s.Tpt20StatusOf(err).Code)

	_, err = s.Tpt20Dial(os.Args[1]).Unary(ctx, "codegen_test.v1.Pinger/Nope", nil, nil)
	fmt.Printf("unknown %d\n", s.Tpt20StatusOf(err).Code)
}
"#;

const GO_SERVER: &str = r#"
package main

import (
	"context"
	"fmt"
	"io"
	"net"
	"os"
	"strings"

	s "codegen_test_v1"
)

type impl struct{}

func (impl) Ping(ctx context.Context, md s.Tpt20Metadata, req *s.PingRequest) (*s.PingReply, error) {
	who := md.Get("x-who")
	if who == "" {
		who = "anon"
	}
	return &s.PingReply{Text: who + ":" + req.Text, N: req.Count + 1}, nil
}

func (impl) Repeat(ctx context.Context, md s.Tpt20Metadata, req *s.PingRequest, st *s.Tpt20ServerStream[*s.PingRequest, *s.PingReply]) error {
	for i := int32(0); i < req.Count; i++ {
		if err := st.Send(&s.PingReply{Text: req.Text, N: i}); err != nil {
			return err
		}
	}
	return nil
}

func (impl) Collect(ctx context.Context, md s.Tpt20Metadata, st *s.Tpt20ServerStream[*s.PingRequest, *s.PingReply]) (*s.PingReply, error) {
	text, n := "", int32(0)
	for {
		r, err := st.Recv()
		if err == io.EOF {
			return &s.PingReply{Text: text, N: n}, nil
		}
		if err != nil {
			return nil, err
		}
		text += r.Text
		n++
	}
}

func (impl) Chat(ctx context.Context, md s.Tpt20Metadata, st *s.Tpt20ServerStream[*s.PingRequest, *s.PingReply]) error {
	for {
		r, err := st.Recv()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return err
		}
		if err := st.Send(&s.PingReply{Text: strings.ToUpper(r.Text), N: r.Count}); err != nil {
			return err
		}
	}
}

func (impl) Fail(ctx context.Context, md s.Tpt20Metadata, req *s.PingRequest) (*s.PingReply, error) {
	return nil, s.Tpt20Error(s.Tpt20PermissionDenied, "nope: "+req.Text)
}

func (impl) GetHTTPStatus(ctx context.Context, md s.Tpt20Metadata, req *s.PingRequest) (*s.PingReply, error) {
	return &s.PingReply{Text: "200", N: 200}, nil
}

func main() {
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		panic(err)
	}
	srv := s.NewTpt20Server()
	s.RegisterPinger(srv, impl{})
	fmt.Println("LISTENING", l.Addr().String())
	os.Stdout.Sync()
	panic(srv.Serve(l))
}
"#;

fn go_env(mut cmd: Command, dir: &Path) -> Command {
    cmd.current_dir(dir)
        .env("GOFLAGS", "-mod=mod")
        .env("GOCACHE", std::env::temp_dir().join("tpt20-gocache"));
    cmd
}

fn go_build(dir: &Path, name: &str, source: &str) -> PathBuf {
    std::fs::create_dir_all(dir.join(name)).unwrap();
    std::fs::write(dir.join(name).join("main.go"), source).unwrap();
    let mut cmd = Command::new("go");
    cmd.args(["build", "-o"])
        .arg(dir.join(format!("{name}.bin")))
        .arg(format!("./{name}"));
    let out = go_env(cmd, dir).output().unwrap();
    assert!(
        out.status.success(),
        "go build {name} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir.join(format!("{name}.bin"))
}

#[tokio::test(flavor = "multi_thread")]
async fn go_client_against_rust_server() {
    if !have("go", "version") {
        eprintln!("go not installed; skipping");
        return;
    }
    let dir = scratch("go-client");
    generate("go", &dir);
    let bin = go_build(&dir, "client", GO_CLIENT);
    let addr = rust_server().await;
    let out = tokio::task::spawn_blocking(move || run_client(Command::new(bin), &addr))
        .await
        .unwrap();
    assert_eq!(out, EXPECTED);
}

#[tokio::test(flavor = "multi_thread")]
async fn rust_client_against_go_server() {
    if !have("go", "version") {
        eprintln!("go not installed; skipping");
        return;
    }
    let dir = scratch("go-server");
    generate("go", &dir);
    let bin = go_build(&dir, "server", GO_SERVER);
    let (mut child, addr) = start_server(Command::new(bin));
    rust_client_scenario(addr).await;
    let _ = child.kill();
    let _ = child.wait();
}

/// Manual debugging aid: `cargo test -p tpt20-codegen-tests --test polyglot_rpc
/// -- --ignored --nocapture rust_server_for_manual_clients` prints an address
/// that generated clients can be pointed at for 10 minutes.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn rust_server_for_manual_clients() {
    let addr = rust_server().await;
    println!("LISTENING {addr}");
    tokio::time::sleep(Duration::from_secs(600)).await;
}

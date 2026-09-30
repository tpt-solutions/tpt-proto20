//! End-to-end tests of generated service code: server trait + wrapper +
//! client stub, over the in-process and HTTP/2 transports.

use futures::{Stream, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tpt20_codegen_tests::generated::{PingReply, PingRequest, Pinger, PingerClient, PingerServer};
use tpt20_rpc::{async_trait, BoxStream, Channel, RpcContext, RpcError, Server, Status};
use tpt20_transport::http2::{Http2Server, Http2Transport};
use tpt20_transport::{Endpoint, InProcessServer};

struct Impl;

fn reply(text: impl Into<String>, n: i32) -> PingReply {
    PingReply {
        text: text.into(),
        n,
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

fn req(text: &str, count: i32) -> PingRequest {
    PingRequest {
        text: text.into(),
        count,
        ..Default::default()
    }
}

async fn exercise(client: PingerClient) {
    let mut ctx = RpcContext::new();
    ctx.metadata_mut().insert_text("x-who", "ada").unwrap();

    // unary (+ metadata)
    let r = client.ping(&ctx, &req("hi", 41)).await.unwrap();
    assert_eq!((r.text.as_str(), r.n), ("ada:hi", 42));

    // server streaming
    let s = client.repeat(&ctx, &req("x", 3)).await.unwrap();
    let got: Vec<_> = s.map(|r| r.unwrap().n).collect().await;
    assert_eq!(got, vec![0, 1, 2]);

    // client streaming
    let r = client
        .collect(
            &ctx,
            futures::stream::iter(vec![req("a", 0), req("b", 0), req("c", 0)]),
        )
        .await
        .unwrap();
    assert_eq!((r.text.as_str(), r.n), ("abc", 3));

    // bidi
    let s = client
        .chat(&ctx, futures::stream::iter(vec![req("p", 1), req("q", 2)]))
        .await
        .unwrap();
    let got: Vec<_> = s.map(|r| r.unwrap()).map(|r| (r.text, r.n)).collect().await;
    assert_eq!(got, vec![("P".to_string(), 1), ("Q".to_string(), 2)]);

    // error status propagates with its message
    let e = client.fail(&ctx, &req("secret", 0)).await.unwrap_err();
    assert_eq!(
        (e.status(), e.message()),
        (Status::PermissionDenied, "nope: secret")
    );

    // acronym method names: snake_case in Rust, original name on the wire
    assert_eq!(
        client.get_http_status(&ctx, &req("", 0)).await.unwrap().n,
        200
    );

    // deadline / cancellation plumb through the generated client
    let done = RpcContext::new().with_timeout(Duration::from_millis(0));
    let e = client.ping(&done, &req("late", 0)).await.unwrap_err();
    assert_eq!(e.status(), Status::DeadlineExceeded);
    let cancelled = RpcContext::new();
    cancelled.cancellation().cancel();
    let e = client.ping(&cancelled, &req("x", 0)).await.unwrap_err();
    assert_eq!(e.status(), Status::Cancelled);
}

fn server() -> Arc<Server> {
    Arc::new(Server::new().add_service(PingerServer::new(Impl)))
}

#[tokio::test]
async fn generated_service_over_in_process_transport() {
    let (srv, rx) = InProcessServer::bind(16);
    tokio::spawn(server().serve_in_process(rx));
    exercise(PingerClient::new(Channel::new(srv.transport()))).await;
}

#[tokio::test]
async fn generated_service_over_http2() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let http2 = Http2Server::new(Endpoint::new(addr.clone()));
    let srv = server();
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
    exercise(PingerClient::new(Channel::new(Http2Transport::new(
        Endpoint::new(addr),
    ))))
    .await;
}

#[test]
fn service_constants_use_package_qualified_names() {
    assert_eq!(PingerClient::NAME, "codegen_test.v1.Pinger");
    assert_eq!(PingerServer::<Impl>::NAME, "codegen_test.v1.Pinger");
}

#[allow(dead_code)]
fn _stream_bound_is_send(s: impl Stream<Item = PingRequest> + Send + 'static) -> impl Send {
    s
}

#[tokio::test]
async fn generated_descriptor_is_served_through_reflection() {
    use tpt20_codegen_tests::generated::{DESCRIPTOR, FINGERPRINT, PACKAGE, SERVICE_NAMES};
    use tpt20_rpc::reflection::{self, ReflectionService};

    assert_eq!(PACKAGE, "codegen_test.v1");
    assert_eq!(SERVICE_NAMES, &["codegen_test.v1.Pinger"]);

    let reflection =
        ReflectionService::new().register(PACKAGE, DESCRIPTOR, FINGERPRINT, SERVICE_NAMES);
    let (srv, rx) = InProcessServer::bind(8);
    let rpc = Arc::new(
        Server::new()
            .add_service(PingerServer::new(Impl))
            .add_service(reflection),
    );
    tokio::spawn(rpc.serve_in_process(rx));
    let ch = Channel::new(srv.transport());
    let ctx = RpcContext::new();

    assert_eq!(
        reflection::list_services(&ch, &ctx).await.unwrap(),
        SERVICE_NAMES
    );
    let remote = reflection::get_descriptor(&ch, &ctx, "").await.unwrap();
    assert_eq!(remote.fingerprint, FINGERPRINT);
    let d = tpt20_descriptor::Descriptor::from_binary(&remote.descriptor).unwrap();
    let svc = d.find_service("Pinger").expect("service in descriptor");
    assert_eq!(svc.methods.len(), 6);
    assert!(d.find_message("Outer").is_some());
}

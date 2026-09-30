//! RPC benchmarks over the in-process and HTTP/2 (plain and TLS) transports,
//! using the generated `Bench` service.

use criterion::{criterion_group, criterion_main, Criterion};
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tpt20_benches::generated::*;
use tpt20_rpc::{async_trait, BoxStream, Channel, RpcContext, RpcError, Server};
use tpt20_transport::http2::{Http2Server, Http2Transport};
use tpt20_transport::{Endpoint, InProcessServer, TlsConfig};

struct Impl;

#[async_trait]
impl Bench for Impl {
    async fn unary(&self, _ctx: &RpcContext, request: Small) -> Result<Small, RpcError> {
        Ok(request)
    }

    async fn stream(
        &self,
        _ctx: &RpcContext,
        request: Small,
    ) -> Result<BoxStream<'static, Result<Small, RpcError>>, RpcError> {
        Ok(futures::stream::iter((0..100).map(move |i| {
            Ok(Small {
                id: i,
                ..request.clone()
            })
        }))
        .boxed())
    }

    async fn slow(&self, _ctx: &RpcContext, request: Small) -> Result<Small, RpcError> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(request)
    }
}

fn server() -> Arc<Server> {
    Arc::new(Server::new().add_service(BenchServer::new(Impl)))
}

fn req() -> Small {
    Small {
        id: 7,
        name: "benchmark".into(),
        flag: true,
        ..Default::default()
    }
}

fn in_process(rt: &Runtime) -> BenchClient {
    let (srv, rx) = InProcessServer::bind(1024);
    rt.spawn(server().serve_in_process(rx));
    BenchClient::new(Channel::new(srv.transport()))
}

fn http2(rt: &Runtime, tls: Option<&rcgen::CertifiedKey>) -> BenchClient {
    let (addr, client_ep) = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut server_ep = Endpoint::new(format!("localhost:{port}"));
        let mut client_ep = Endpoint::new(format!("localhost:{port}"));
        if let Some(cert) = tls {
            let mut cfg = TlsConfig::http2();
            cfg.cert_pem = Some(cert.cert.pem().into_bytes());
            cfg.key_pem = Some(cert.key_pair.serialize_pem().into_bytes());
            server_ep = server_ep.with_tls(cfg.clone());
            let mut client_cfg = cfg;
            client_cfg.key_pem = None;
            client_ep = client_ep.with_tls(client_cfg);
        }
        let srv = server();
        let http = Http2Server::new(server_ep);
        tokio::spawn(async move {
            let _ = http
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
        (port, client_ep)
    });
    let _ = addr;
    BenchClient::new(Channel::new(Http2Transport::new(client_ep)))
}

fn unary_and_streaming(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let ctx = RpcContext::new();
    let request = req();

    let mut g = c.benchmark_group("rpc");
    g.sample_size(30);
    g.measurement_time(Duration::from_secs(3));

    let inproc = in_process(&rt);
    g.bench_function("unary/in_process", |b| {
        b.to_async(&rt)
            .iter(|| async { inproc.unary(&ctx, &request).await.unwrap() })
    });
    g.bench_function("streaming_100/in_process", |b| {
        b.to_async(&rt)
            .iter(|| async { inproc.stream(&ctx, &request).await.unwrap().count().await })
    });

    let plain = http2(&rt, None);
    g.bench_function("unary/http2_h2c", |b| {
        b.to_async(&rt)
            .iter(|| async { plain.unary(&ctx, &request).await.unwrap() })
    });
    g.bench_function("streaming_100/http2_h2c", |b| {
        b.to_async(&rt)
            .iter(|| async { plain.stream(&ctx, &request).await.unwrap().count().await })
    });

    // TLS overhead: same call, TLS 1.3 + ALPN (a handshake per call, since
    // the client opens a connection per call).
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let tls = http2(&rt, Some(&cert));
    g.bench_function("unary/http2_tls", |b| {
        b.to_async(&rt)
            .iter(|| async { tls.unary(&ctx, &request).await.unwrap() })
    });
    g.finish();
}

fn concurrency(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("rpc_concurrency");
    g.sample_size(20);
    g.measurement_time(Duration::from_secs(3));
    let request = req();

    for (name, client) in [
        ("in_process", in_process(&rt)),
        ("http2_h2c", http2(&rt, None)),
    ] {
        g.bench_function(format!("32_concurrent_unary/{name}"), |b| {
            b.to_async(&rt).iter(|| async {
                let ctx = RpcContext::new();
                let calls = (0..32).map(|_| client.unary(&ctx, &request));
                futures::future::join_all(calls).await
            })
        });
    }

    // Storms against a handler that never answers in time.
    let client = in_process(&rt);
    g.bench_function("cancellation_storm_100/in_process", |b| {
        b.to_async(&rt).iter(|| async {
            let ctxs: Vec<RpcContext> = (0..100).map(|_| RpcContext::new()).collect();
            let calls = ctxs.iter().map(|ctx| client.slow(ctx, &request));
            let cancel = async {
                tokio::task::yield_now().await;
                for ctx in &ctxs {
                    ctx.cancellation().cancel();
                }
            };
            let (results, ()) = futures::join!(futures::future::join_all(calls), cancel);
            assert!(results.iter().all(Result::is_err));
        })
    });
    g.bench_function("deadline_storm_100/in_process", |b| {
        b.to_async(&rt).iter(|| async {
            let ctxs: Vec<RpcContext> = (0..100)
                .map(|_| RpcContext::new().with_timeout(Duration::from_millis(1)))
                .collect();
            let calls = ctxs.iter().map(|ctx| client.slow(ctx, &request));
            let results = futures::future::join_all(calls).await;
            assert!(results.iter().all(Result::is_err));
        })
    });
    g.finish();
}

criterion_group!(benches, unary_and_streaming, concurrency);
criterion_main!(benches);

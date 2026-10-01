//! Runtime tests: hand-written service over the in-process and HTTP/2
//! transports exercising status, metadata, deadlines, cancellation, and all
//! four streaming shapes.

use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tpt20_core::DecodeError;
use tpt20_rpc::{async_trait, Channel, RpcContext, RpcError, Server, ServerCall, Service, Status};
use tpt20_transport::http2::{Http2Server, Http2Transport};
use tpt20_transport::Compression;
use tpt20_transport::{
    Endpoint, InProcessServer, Metadata as WireMetadata, StreamingType, Transport,
};

fn dec(b: &[u8]) -> Result<Vec<u8>, DecodeError> {
    Ok(b.to_vec())
}
// Must take `&Vec<u8>`: the response type is `Vec<u8>`.
#[allow(clippy::ptr_arg)]
fn enc(v: &Vec<u8>) -> Vec<u8> {
    v.clone()
}

static HANDLERS_DROPPED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Counts the handler future being dropped (i.e. its work being cancelled).
struct DropGuard;

impl Drop for DropGuard {
    fn drop(&mut self) {
        HANDLERS_DROPPED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

struct Echo;

#[async_trait]
impl Service for Echo {
    fn name(&self) -> &'static str {
        "echo.Echo"
    }

    async fn handle(&self, method: &str, call: ServerCall) {
        match method {
            "Upper" => {
                call.unary(
                    dec,
                    enc,
                    |_ctx, req| async move { Ok(req.to_ascii_uppercase()) },
                )
                .await
            }
            "Fail" => {
                call.unary(dec, enc, |_ctx, _req: Vec<u8>| async move {
                    Err::<Vec<u8>, _>(RpcError::not_found("no such thing: é").finish())
                })
                .await
            }
            "Meta" => {
                call.unary(dec, enc, |ctx, _req: Vec<u8>| async move {
                    let user = ctx
                        .metadata()
                        .get_first_text("x-user")
                        .unwrap_or("-")
                        .to_string();
                    let blob = match ctx.metadata().get("x-blob-bin") {
                        Some(tpt20_rpc::MetadataValue::Binary(b)) => b.clone(),
                        _ => vec![],
                    };
                    let mut out = user.into_bytes();
                    out.push(b'|');
                    out.extend(blob);
                    Ok(out)
                })
                .await
            }
            "Count" => {
                call.server_streaming(dec, enc, |_ctx, req: Vec<u8>| async move {
                    let n = req[0] as usize;
                    let s: BoxStream<'static, Result<Vec<u8>, RpcError>> =
                        futures::stream::iter((0..n).map(|i| Ok(vec![i as u8]))).boxed();
                    Ok(s)
                })
                .await
            }
            "CountThenFail" => {
                call.server_streaming(dec, enc, |_ctx, _req: Vec<u8>| async move {
                    let s: BoxStream<'static, Result<Vec<u8>, RpcError>> =
                        futures::stream::iter(vec![
                            Ok(vec![1]),
                            Err(RpcError::aborted("stopped").finish()),
                        ])
                        .boxed();
                    Ok(s)
                })
                .await
            }
            "Sum" => {
                call.client_streaming(dec, enc, |_ctx, mut reqs| async move {
                    let mut total = 0u32;
                    while let Some(r) = reqs.next().await {
                        total += r?.iter().map(|b| u32::from(*b)).sum::<u32>();
                    }
                    Ok(total.to_be_bytes().to_vec())
                })
                .await
            }
            "Chat" => {
                call.bidi(dec, enc, |_ctx, reqs| async move {
                    let s: BoxStream<'static, Result<Vec<u8>, RpcError>> = reqs
                        .map(|r| {
                            r.map(|mut v| {
                                v.insert(0, b'>');
                                v
                            })
                        })
                        .boxed();
                    Ok(s)
                })
                .await
            }
            "Slow" => {
                call.unary(dec, enc, |_ctx, _req: Vec<u8>| async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    Ok(vec![])
                })
                .await
            }
            "Guarded" => {
                call.unary(dec, enc, |_ctx, _req: Vec<u8>| async move {
                    let _guard = DropGuard;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    Ok(vec![])
                })
                .await
            }
            "Panic" => {
                call.unary(dec, enc, |_ctx, _req: Vec<u8>| async move {
                    if true {
                        panic!("handler bug");
                    }
                    Ok(vec![])
                })
                .await
            }
            "Forgets" => {
                // Returns without completing the call.
                let _ = call;
            }
            other => {
                call.finish(Err(
                    RpcError::unimplemented(format!("no method {other}")).finish()
                ))
                .await
            }
        }
    }
}

fn server() -> Arc<Server> {
    Arc::new(Server::new().add_service(Echo))
}

async fn in_process_channel() -> Channel {
    let (srv, rx) = InProcessServer::bind(16);
    tokio::spawn(server().serve_in_process(rx));
    Channel::new(srv.transport())
}

async fn http2_channel() -> (Channel, WireEndpoint) {
    http2_channel_with(None).await
}

async fn http2_channel_with(compression: Option<Compression>) -> (Channel, WireEndpoint) {
    let with = |ep: Endpoint| match compression {
        Some(c) => ep.with_compression(c, 0),
        None => ep,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let http2 = Http2Server::new(with(Endpoint::new(addr.clone())));
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
    (
        Channel::new(Http2Transport::new(with(Endpoint::new(addr.clone())))),
        WireEndpoint(addr),
    )
}

struct WireEndpoint(String);

#[cfg(feature = "quic")]
async fn quic_channel() -> Channel {
    use tpt20_transport::quic::{QuicServer, QuicTransport};
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let mut tls = tpt20_transport::TlsConfig::http2();
    tls.cert_pem = Some(cert.cert.pem().into_bytes());
    tls.key_pem = Some(cert.key_pair.serialize_pem().into_bytes());
    let server_ep = Endpoint::new("127.0.0.1:0").with_tls(tls.clone());
    let quic = QuicServer::new(server_ep);
    let socket = quic.bind().unwrap();
    let port = socket.local_addr().unwrap().port();
    let srv = server();
    tokio::spawn(async move {
        let _ = quic
            .serve_socket(
                socket,
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
    Channel::new(QuicTransport::new(
        Endpoint::new(format!("127.0.0.1:{port}")).with_tls(tls.with_server_name("localhost")),
    ))
}

fn reqs<T: Send + 'static>(items: Vec<T>) -> BoxStream<'static, T> {
    futures::stream::iter(items).boxed()
}

async fn collect_ok<T>(s: impl Stream<Item = Result<T, RpcError>>) -> Vec<T> {
    s.map(|r| r.unwrap()).collect().await
}

/// The full behavioral suite, run against every transport.
async fn suite(ch: Channel) {
    let ctx = RpcContext::new();

    // unary
    assert_eq!(
        ch.unary("echo.Echo/Upper", &ctx, b"abc".to_vec(), dec)
            .await
            .unwrap(),
        b"ABC"
    );

    // error status + percent-encoded message survive the wire
    let e = ch
        .unary("echo.Echo/Fail", &ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(
        (e.status(), e.message()),
        (Status::NotFound, "no such thing: é")
    );

    // unknown service / method
    let e = ch.unary("nope.Svc/X", &ctx, vec![], dec).await.unwrap_err();
    assert_eq!(e.status(), Status::Unimplemented);
    let e = ch
        .unary("echo.Echo/Missing", &ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::Unimplemented);

    // metadata (text + binary) reaches the handler
    let mut md_ctx = RpcContext::new();
    md_ctx.metadata_mut().insert_text("x-user", "ada").unwrap();
    md_ctx
        .metadata_mut()
        .insert_binary("x-blob-bin", vec![0u8, 255, 1])
        .unwrap();
    assert_eq!(
        ch.unary("echo.Echo/Meta", &md_ctx, vec![], dec)
            .await
            .unwrap(),
        [b"ada|".as_slice(), &[0, 255, 1]].concat()
    );

    // server streaming, including an error after some messages
    let s = ch
        .server_streaming("echo.Echo/Count", &ctx, vec![4], dec)
        .await
        .unwrap();
    assert_eq!(
        collect_ok(s).await,
        vec![vec![0], vec![1], vec![2], vec![3]]
    );
    let s = ch
        .server_streaming("echo.Echo/Count", &ctx, vec![0], dec)
        .await
        .unwrap();
    assert!(collect_ok(s).await.is_empty());
    let items: Vec<_> = ch
        .server_streaming("echo.Echo/CountThenFail", &ctx, vec![], dec)
        .await
        .unwrap()
        .collect()
        .await;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].as_ref().unwrap(), &vec![1]);
    assert_eq!(items[1].as_ref().unwrap_err().status(), Status::Aborted);

    // client streaming (empty and non-empty)
    let r = ch
        .client_streaming(
            "echo.Echo/Sum",
            &ctx,
            reqs(vec![vec![1, 2], vec![3], vec![4]]),
            dec,
        )
        .await
        .unwrap();
    assert_eq!(r, 10u32.to_be_bytes());
    let r = ch
        .client_streaming("echo.Echo/Sum", &ctx, reqs(Vec::<Vec<u8>>::new()), dec)
        .await
        .unwrap();
    assert_eq!(r, 0u32.to_be_bytes());

    // bidi
    let s = ch
        .bidi(
            "echo.Echo/Chat",
            &ctx,
            reqs(vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]),
            dec,
        )
        .await
        .unwrap();
    assert_eq!(
        collect_ok(s).await,
        vec![b">a".to_vec(), b">b".to_vec(), b">c".to_vec()]
    );

    // deadline: client gives up; the server-side timeout is covered below
    let short = RpcContext::new().with_timeout(Duration::from_millis(150));
    let t = std::time::Instant::now();
    let e = ch
        .unary("echo.Echo/Slow", &short, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::DeadlineExceeded);
    assert!(t.elapsed() < Duration::from_secs(3));

    // cancellation
    let cancel_ctx = RpcContext::new();
    let token = cancel_ctx.cancellation().clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        token.cancel();
    });
    let e = ch
        .unary("echo.Echo/Slow", &cancel_ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::Cancelled);
    let e = ch
        .unary("echo.Echo/Upper", &cancel_ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(
        e.status(),
        Status::Cancelled,
        "already-cancelled calls never start"
    );

    // handler bugs never look like success
    let e = ch
        .unary("echo.Echo/Panic", &ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::Internal);
    let e = ch
        .unary("echo.Echo/Forgets", &ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::Internal);
}

#[tokio::test]
async fn behavior_over_in_process_transport() {
    suite(in_process_channel().await).await;
}

#[tokio::test]
async fn behavior_over_http2_transport() {
    let (ch, _addr) = http2_channel().await;
    suite(ch).await;
}

#[cfg(feature = "quic")]
#[tokio::test]
async fn behavior_over_quic_transport() {
    suite(quic_channel().await).await;
}

/// When the client gives up on a call, the server stops the handler instead
/// of letting it run to completion for nobody.
#[tokio::test]
async fn client_cancellation_stops_the_server_handler() {
    use std::sync::atomic::Ordering;
    let (http2_ch, _addr) = http2_channel().await;
    #[allow(unused_mut)]
    let mut channels = vec![
        ("http2", http2_ch),
        ("in-process", in_process_channel().await),
    ];
    #[cfg(feature = "quic")]
    channels.push(("quic", quic_channel().await));
    for (name, ch) in channels {
        let before = HANDLERS_DROPPED.load(Ordering::SeqCst);
        let ctx = RpcContext::new();
        let token = ctx.cancellation().clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            token.cancel();
        });
        let e = ch
            .unary("echo.Echo/Guarded", &ctx, vec![], dec)
            .await
            .unwrap_err();
        assert_eq!(e.status(), Status::Cancelled, "{name}");
        let mut stopped = false;
        for _ in 0..100 {
            if HANDLERS_DROPPED.load(Ordering::SeqCst) > before {
                stopped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            stopped,
            "{name}: server handler kept running after the client cancelled"
        );
    }
}

#[tokio::test]
async fn behavior_over_compressed_http2() {
    for alg in [Compression::Gzip, Compression::Deflate] {
        let (ch, _addr) = http2_channel_with(Some(alg)).await;
        suite(ch).await;
    }
}

#[tokio::test]
async fn server_enforces_the_clients_deadline() {
    let (_ch, WireEndpoint(addr)) = http2_channel().await;
    // Bypass Channel so the client does not time out on its own.
    let transport = Http2Transport::new(Endpoint::new(addr));
    let mut md = WireMetadata::new();
    md.insert("grpc-timeout", "100m");
    let call = transport
        .start_call("echo.Echo/Slow", vec![], &md, StreamingType::Unary)
        .await
        .unwrap();
    let items: Vec<_> = call.stream.collect().await;
    let trailers = items
        .into_iter()
        .filter_map(|i| match i.unwrap() {
            tpt20_transport::StreamItem::Trailer(t) => Some(t),
            _ => None,
        })
        .next()
        .expect("trailers");
    let err = tpt20_rpc::wire::parse_status(&trailers).unwrap_err();
    assert_eq!(err.status(), Status::DeadlineExceeded);
}

#[tokio::test]
async fn unreachable_server_is_unavailable() {
    let ch = Channel::new(Http2Transport::new(Endpoint::new("127.0.0.1:1")));
    let e = ch
        .unary("echo.Echo/Upper", &RpcContext::new(), vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::Unavailable);
}

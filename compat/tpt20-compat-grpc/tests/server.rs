//! `GrpcServer` spoken to by a plain HTTP/2 client using stock gRPC framing
//! and headers (no tpt20 client code on the wire).
#![cfg(feature = "server")]

use bytes::{Buf, Bytes};
use http::Request;
use tpt20_compat_grpc::{GrpcError, GrpcServer};
use tpt20_transport::Endpoint;

struct Reply {
    content_type: String,
    messages: Vec<Vec<u8>>,
    trailers: http::HeaderMap,
}

async fn spawn_server() -> (String, tokio::sync::oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = GrpcServer::new(Endpoint::new(addr.clone()));
    tokio::spawn(async move {
        let _ = server
            .serve_listener(
                listener,
                |mut call| async move {
                    match call.method.as_str() {
                        "demo.v1.Echo/Say" => {
                            let mut out = b"echo:".to_vec();
                            out.extend_from_slice(&call.payload);
                            call.send_ok(out).await
                        }
                        "demo.v1.Echo/Denied" => {
                            call.send_status(tpt20_rpc::Status::PermissionDenied, "no way")
                                .await
                        }
                        "demo.v1.Echo/Count" => {
                            for i in 0..3u8 {
                                call.send_ok(vec![i]).await?;
                            }
                            Ok(())
                        }
                        _ => Err(GrpcError::NotSupported("unknown".into())),
                    }
                },
                async {
                    let _ = stop_rx.await;
                },
            )
            .await;
    });
    (addr, stop_tx)
}

async fn call(addr: &str, path: &str, payload: &[u8]) -> Reply {
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (mut client, conn) = h2::client::handshake(tcp).await.unwrap();
    tokio::spawn(conn);
    let req = Request::post(format!("http://{addr}/{path}"))
        .header("content-type", "application/grpc+proto")
        .header("te", "trailers")
        .header("user-agent", "grpc-rust/0.0")
        .body(())
        .unwrap();
    let (resp, mut send) = client.send_request(req, false).unwrap();
    let mut frame = vec![0u8];
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    send.send_data(Bytes::from(frame), true).unwrap();

    let resp = resp.await.unwrap();
    assert_eq!(resp.status(), 200);
    let content_type = resp.headers()["content-type"].to_str().unwrap().to_string();
    let mut body = resp.into_body();
    let mut buf = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.unwrap();
        body.flow_control().release_capacity(chunk.len()).unwrap();
        buf.extend_from_slice(&chunk);
    }
    let trailers = body.trailers().await.unwrap().expect("trailers");
    let mut messages = Vec::new();
    let mut cur = Bytes::from(buf);
    while cur.has_remaining() {
        assert_eq!(cur.get_u8(), 0, "uncompressed frame expected");
        let len = cur.get_u32() as usize;
        messages.push(cur.split_to(len).to_vec());
    }
    Reply {
        content_type,
        messages,
        trailers,
    }
}

#[tokio::test]
async fn unary_call_from_a_stock_grpc_client() {
    let (addr, _stop) = spawn_server().await;
    let r = call(&addr, "demo.v1.Echo/Say", b"hi").await;
    assert_eq!(r.content_type, "application/grpc+proto");
    assert_eq!(r.messages, vec![b"echo:hi".to_vec()]);
    assert_eq!(r.trailers["grpc-status"], "0");
}

#[tokio::test]
async fn status_only_reply_carries_code_and_message() {
    let (addr, _stop) = spawn_server().await;
    let r = call(&addr, "demo.v1.Echo/Denied", b"").await;
    assert!(r.messages.is_empty());
    assert_eq!(r.trailers["grpc-status"], "7");
    assert_eq!(r.trailers["grpc-message"], "no way");
}

#[tokio::test]
async fn server_streaming_and_implicit_ok() {
    let (addr, _stop) = spawn_server().await;
    let r = call(&addr, "demo.v1.Echo/Count", b"").await;
    assert_eq!(r.messages, vec![vec![0], vec![1], vec![2]]);
    assert_eq!(r.trailers["grpc-status"], "0");
}

#[tokio::test]
async fn handler_error_becomes_internal() {
    let (addr, _stop) = spawn_server().await;
    let r = call(&addr, "demo.v1.Echo/Nope", b"").await;
    assert_eq!(r.trailers["grpc-status"], "13");
    assert!(r.trailers["grpc-message"]
        .to_str()
        .unwrap()
        .contains("unknown"));
}

//! Reflection descriptors are validated by an independent protobuf
//! implementation (`prost-reflect`), then served to a plain HTTP/2 client.
#![cfg(all(feature = "server", feature = "reflection"))]

use bytes::{Buf, Bytes};
use http::Request;
use prost_reflect::{Cardinality, DescriptorPool, Kind};
use tpt20_compat_grpc::reflection_wire::{file_descriptor_bytes, ReflectionServer};
use tpt20_compat_grpc::GrpcServer;
use tpt20_transport::Endpoint;

const SCHEMA: &str = r#"
package demo.v1;

enum Color { RED = 0; GREEN = 1; }

message Item {
  1: name string;
  2: qty int32;
  3: tags repeated string;
  4: attrs map<string, int64>;
  5: color Color;
  6: note string?;
  7: sub Item.Part;
  message Part { 1: id sint64; 2: blob bytes; }
  oneof pick { 8: a uint32; 9: b Item; }
}

message Empty {}

service Shop {
  Get(Empty) returns (Item);
  Watch(Empty) returns (stream Item);
  Upload(stream Item) returns (Empty);
}
"#;

fn package() -> tpt20_ir::PackageIr {
    tpt20_compiler::pipeline::compile(SCHEMA, None)
        .map_err(|d| format!("{d:?}"))
        .unwrap()
        .ir
}

fn pool_from(file: &[u8]) -> DescriptorPool {
    let mut set = vec![0x0a];
    let mut len = file.len();
    while len >= 0x80 {
        set.push((len as u8) | 0x80);
        len >>= 7;
    }
    set.push(len as u8);
    set.extend_from_slice(file);
    DescriptorPool::decode(set.as_slice()).expect("valid FileDescriptorSet")
}

#[test]
fn generated_file_descriptor_is_valid_protobuf_schema() {
    let pool = pool_from(&file_descriptor_bytes("demo/v1/demo.proto", &package()));
    let item = pool.get_message_by_name("demo.v1.Item").unwrap();
    assert_eq!(item.get_field_by_name("name").unwrap().kind(), Kind::String);
    assert_eq!(item.get_field_by_name("qty").unwrap().number(), 2);
    let tags = item.get_field_by_name("tags").unwrap();
    assert_eq!(tags.cardinality(), Cardinality::Repeated);
    assert!(item.get_field_by_name("attrs").unwrap().is_map());
    match item.get_field_by_name("color").unwrap().kind() {
        Kind::Enum(e) => assert_eq!(e.full_name(), "demo.v1.Color"),
        other => panic!("{other:?}"),
    }
    assert!(item.get_field_by_name("note").unwrap().supports_presence());
    match item.get_field_by_name("sub").unwrap().kind() {
        Kind::Message(m) => assert_eq!(m.full_name(), "demo.v1.Item.Part"),
        other => panic!("{other:?}"),
    }
    let a = item.get_field_by_name("a").unwrap();
    assert!(a.containing_oneof().is_some_and(|o| o.name() == "pick"));
    let part = pool.get_message_by_name("demo.v1.Item.Part").unwrap();
    assert_eq!(part.get_field_by_name("id").unwrap().kind(), Kind::Sint64);

    let shop = pool.get_service_by_name("demo.v1.Shop").unwrap();
    let methods: Vec<_> = shop.methods().collect();
    assert_eq!(methods.len(), 3);
    assert!(!methods[0].is_server_streaming());
    assert!(methods[1].is_server_streaming());
    assert!(methods[2].is_client_streaming());
    assert_eq!(methods[0].output().full_name(), "demo.v1.Item");
}

fn request(field: u32, value: &str) -> Vec<u8> {
    let mut out = vec![((field << 3) | 2) as u8, value.len() as u8];
    out.extend_from_slice(value.as_bytes());
    out
}

#[test]
fn respond_handles_list_symbol_and_errors() {
    let server = ReflectionServer::new().with_package("demo/v1/demo.proto", &package());
    assert_eq!(server.service_names(), vec!["demo.v1.Shop"]);

    let list = server.respond(&request(7, "*"));
    assert!(windows(&list, b"demo.v1.Shop"));

    let sym = server.respond(&request(4, "demo.v1.Item"));
    assert!(windows(&sym, b"demo/v1/demo.proto"));

    let by_method = server.respond(&request(4, "demo.v1.Shop.Watch"));
    assert!(windows(&by_method, b"demo/v1/demo.proto"));

    let missing = server.respond(&request(4, "nope.Nothing"));
    assert!(windows(&missing, b"symbol not found"));

    let garbage = server.respond(&[0xff, 0xff, 0xff]);
    assert!(windows(&garbage, b"malformed"));
}

fn windows(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

async fn roundtrip(addr: &str, requests: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (mut client, conn) = h2::client::handshake(tcp).await.unwrap();
    tokio::spawn(conn);
    let req = Request::post(format!(
        "http://{addr}/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo"
    ))
    .header("content-type", "application/grpc")
    .header("te", "trailers")
    .body(())
    .unwrap();
    let (resp, mut send) = client.send_request(req, false).unwrap();
    for (i, r) in requests.iter().enumerate() {
        let mut frame = vec![0u8];
        frame.extend_from_slice(&(r.len() as u32).to_be_bytes());
        frame.extend_from_slice(r);
        send.send_data(Bytes::from(frame), i + 1 == requests.len())
            .unwrap();
    }
    let mut body = resp.await.unwrap().into_body();
    let mut buf = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.unwrap();
        body.flow_control().release_capacity(chunk.len()).unwrap();
        buf.extend_from_slice(&chunk);
    }
    let trailers = body.trailers().await.unwrap().unwrap();
    assert_eq!(trailers["grpc-status"], "0");
    let mut cur = Bytes::from(buf);
    let mut out = Vec::new();
    while cur.has_remaining() {
        cur.get_u8();
        let len = cur.get_u32() as usize;
        out.push(cur.split_to(len).to_vec());
    }
    out
}

#[tokio::test]
async fn reflection_stream_over_the_network() {
    let reflection = ReflectionServer::new().with_package("demo/v1/demo.proto", &package());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let server = GrpcServer::new(Endpoint::new(addr.clone()));
    tokio::spawn(async move {
        let _ = server
            .serve_listener(
                listener,
                move |call| {
                    let reflection = reflection.clone();
                    async move {
                        if ReflectionServer::handles(&call.method) {
                            reflection.serve_call(call).await
                        } else {
                            Err(tpt20_compat_grpc::GrpcError::NotSupported("x".into()))
                        }
                    }
                },
                std::future::pending(),
            )
            .await;
    });

    // Both requests ride on one bidi stream, like `grpcurl` does.
    let replies = roundtrip(&addr, &[request(7, "*"), request(4, "demo.v1.Shop")]).await;
    assert_eq!(replies.len(), 2);
    assert!(windows(&replies[0], b"demo.v1.Shop"));
    assert!(windows(&replies[1], b"demo/v1/demo.proto"));
}

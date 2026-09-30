//! Run with `cargo run -p hello-rpc`.
//!
//! Serves the generated `Greeter` over HTTP/2 on a loopback port, calls it
//! with the generated client, and shows the JSON and text representations.

use futures::StreamExt;
use hello_rpc::generated::{GreeterClient, GreeterServer, HelloReply, HelloRequest};
use hello_rpc::{GreeterImpl, SCHEMA};
use std::sync::Arc;
use std::time::Duration;
use tpt20_rpc::{Channel, RpcContext, Server};
use tpt20_transport::http2::{Http2Server, Http2Transport};
use tpt20_transport::Endpoint;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- server -----------------------------------------------------------
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?.to_string();
    let rpc = Arc::new(Server::new().add_service(GreeterServer::new(GreeterImpl)));
    let http2 = Http2Server::new(Endpoint::new(addr.clone()));
    tokio::spawn(async move {
        let _ = http2
            .serve_listener(
                listener,
                move |call| {
                    let rpc = rpc.clone();
                    Box::pin(async move {
                        rpc.handle_call(call).await;
                        Ok(())
                    })
                },
                std::future::pending(),
            )
            .await;
    });
    println!("serving hello.v1.Greeter on {addr}");

    // --- client -----------------------------------------------------------
    let client = GreeterClient::new(Channel::new(Http2Transport::new(Endpoint::new(addr))));
    let mut ctx = RpcContext::new().with_timeout(Duration::from_secs(5));
    ctx.metadata_mut().insert_text("x-lang", "fr")?;

    let request = HelloRequest {
        name: "Ada".into(),
        languages: vec!["en".into(), "es".into(), "fr".into()],
        ..Default::default()
    };
    let reply = client.say_hello(&ctx, &request).await?;
    println!(
        "unary:  {} (lucky number {:?})",
        reply.greeting, reply.lucky_number
    );

    let mut replies = client.say_hello_in_each(&ctx, &request).await?;
    while let Some(r) = replies.next().await {
        println!("stream: {}", r?.greeting);
    }

    let err = client
        .say_hello(&ctx, &HelloRequest::default())
        .await
        .unwrap_err();
    println!("error:  {err}");

    // --- other representations -------------------------------------------
    println!("\nJSON:   {}", reply.to_json()?);
    let bytes = reply.encode();
    println!(
        "wire:   {} bytes, round-trips: {}",
        bytes.len(),
        HelloReply::decode(&bytes)? == reply
    );

    let compiled =
        tpt20_compiler::compile(SCHEMA, None).map_err(|d| tpt20_compiler::render_all(&d))?;
    let descriptor = tpt20_descriptor::Descriptor::new(compiled.ir);
    let text = tpt20_text::TextFormat::new(&descriptor).print_bytes("HelloReply", &bytes)?;
    println!("text:\n{text}");
    Ok(())
}

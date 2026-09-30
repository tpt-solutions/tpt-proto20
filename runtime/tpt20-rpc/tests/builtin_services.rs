//! The built-in health and reflection services, over both transports.

use std::sync::Arc;
use tpt20_rpc::health::{self, HealthService, ServingStatus};
use tpt20_rpc::reflection::{self, ReflectionService};
use tpt20_rpc::{Channel, RpcContext, Server, Status};
use tpt20_transport::http2::{Http2Server, Http2Transport};
use tpt20_transport::{Endpoint, InProcessServer};

const SCHEMA: &str =
    "package demo.v1;\nmessage Ping { 1: n int64; }\nservice Pinger { Do(Ping) returns (Ping); }\n";

fn descriptor() -> (Vec<u8>, String) {
    let compiled = tpt20_compiler::compile(SCHEMA, None).unwrap();
    let mut d = tpt20_descriptor::Descriptor::new(compiled.ir);
    let fp = d.compute_fingerprint();
    (d.to_binary().unwrap(), fp)
}

fn server() -> (Arc<Server>, health::HealthReporter) {
    let (health, reporter) = HealthService::new();
    let (bytes, fp) = descriptor();
    let reflection = ReflectionService::new().register("demo.v1", &bytes, fp, &["demo.v1.Pinger"]);
    (
        Arc::new(Server::new().add_service(health).add_service(reflection)),
        reporter,
    )
}

async fn channels(server: Arc<Server>) -> Vec<(&'static str, Channel)> {
    let (srv, rx) = InProcessServer::bind(8);
    tokio::spawn(server.clone().serve_in_process(rx));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let http2 = Http2Server::new(Endpoint::new(addr.clone()));
    tokio::spawn(async move {
        let _ = http2
            .serve_listener(
                listener,
                move |call| {
                    let s = server.clone();
                    Box::pin(async move {
                        s.handle_call(call).await;
                        Ok(())
                    })
                },
                std::future::pending(),
            )
            .await;
    });
    vec![
        ("in-process", Channel::new(srv.transport())),
        (
            "http2",
            Channel::new(Http2Transport::new(Endpoint::new(addr))),
        ),
    ]
}

#[tokio::test]
async fn health_reports_configured_statuses() {
    let (server, reporter) = server();
    let ctx = RpcContext::new();
    for (name, ch) in channels(server).await {
        // A service name unique per transport: statuses persist in the reporter.
        let svc = format!("demo.v1.Pinger.{name}");
        let svc = svc.as_str();
        // Whole server defaults to SERVING; unknown names are SERVICE_UNKNOWN.
        assert_eq!(
            health::check(&ch, &ctx, "").await.unwrap(),
            ServingStatus::Serving,
            "{name}"
        );
        assert_eq!(
            health::check(&ch, &ctx, svc).await.unwrap(),
            ServingStatus::ServiceUnknown,
            "{name}"
        );
        reporter.set_status(svc, ServingStatus::NotServing);
        assert_eq!(
            health::check(&ch, &ctx, svc).await.unwrap(),
            ServingStatus::NotServing,
            "{name}"
        );
        reporter.set_status(svc, ServingStatus::Serving);
        reporter.set_status("", ServingStatus::NotServing);
        assert_eq!(
            health::check(&ch, &ctx, "").await.unwrap(),
            ServingStatus::NotServing,
            "{name}"
        );
        assert_eq!(
            health::check(&ch, &ctx, svc).await.unwrap(),
            ServingStatus::Serving,
            "{name}"
        );
        reporter.set_status("", ServingStatus::Serving);

        // Unknown method on the service is UNIMPLEMENTED.
        let e = ch
            .unary(
                "tpt20.health.v1.Health/Watch",
                &ctx,
                vec![],
                |b: &[u8]| Ok::<_, tpt20_core::DecodeError>(b.to_vec()),
            )
            .await
            .unwrap_err();
        assert_eq!(e.status(), Status::Unimplemented, "{name}");
    }
}

#[tokio::test]
async fn reflection_lists_services_and_serves_the_descriptor() {
    let (server, _reporter) = server();
    let ctx = RpcContext::new();
    let (expected_bytes, expected_fp) = descriptor();
    for (name, ch) in channels(server).await {
        assert_eq!(
            reflection::list_services(&ch, &ctx).await.unwrap(),
            vec!["demo.v1.Pinger".to_string()],
            "{name}"
        );
        for package in ["", "demo.v1"] {
            let remote = reflection::get_descriptor(&ch, &ctx, package)
                .await
                .unwrap();
            assert_eq!(remote.descriptor, expected_bytes, "{name}/{package}");
            assert_eq!(remote.fingerprint, expected_fp, "{name}/{package}");
            // The bytes are a usable descriptor.
            let d = tpt20_descriptor::Descriptor::from_binary(&remote.descriptor).unwrap();
            assert!(d.find_message("Ping").is_some());
            assert!(d.find_service("Pinger").is_some());
        }
        let e = reflection::get_descriptor(&ch, &ctx, "other.v9")
            .await
            .unwrap_err();
        assert_eq!(e.status(), Status::NotFound, "{name}");
    }
}

#[tokio::test]
async fn ambiguous_or_empty_reflection_requests_are_errors() {
    let ctx = RpcContext::new();
    let (bytes, fp) = descriptor();
    let two = ReflectionService::new()
        .register("a.v1", &bytes, fp.clone(), &["a.v1.S"])
        .register("b.v1", &bytes, fp, &["b.v1.S"]);
    for (name, ch) in channels(Arc::new(Server::new().add_service(two))).await {
        let e = reflection::get_descriptor(&ch, &ctx, "").await.unwrap_err();
        assert_eq!(e.status(), Status::InvalidArgument, "{name}");
        assert_eq!(
            reflection::list_services(&ch, &ctx).await.unwrap().len(),
            2,
            "{name}"
        );
    }
    let empty = Arc::new(Server::new().add_service(ReflectionService::new()));
    for (name, ch) in channels(empty).await {
        let e = reflection::get_descriptor(&ch, &ctx, "").await.unwrap_err();
        assert_eq!(e.status(), Status::NotFound, "{name}");
    }
}

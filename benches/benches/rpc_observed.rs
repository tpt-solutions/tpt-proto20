//! In-process unary RPC with metrics and a logger registered, to compare with
//! `rpc/unary/in_process` (which runs with no observability backend and so
//! measures the inert path). The hooks are process-global and set-once, hence
//! a separate benchmark binary.

use criterion::{criterion_group, criterion_main, Criterion};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::runtime::Runtime;
use tpt20_benches::generated::*;
use tpt20_observability::{
    set_global_logger, set_global_metrics, Labels, LogEvent, Logger, Metrics,
};
use tpt20_rpc::{async_trait, BoxStream, Channel, RpcContext, RpcError, Server};
use tpt20_transport::InProcessServer;

/// A backend that does the minimum a real one must: touch an atomic per event.
struct Counting(AtomicU64);

impl Metrics for Counting {
    fn requests_started(&self, _: &Labels) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn requests_completed(&self, _: &Labels) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn request_duration(&self, _: &Labels, _: Duration) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn active_streams(&self, _: &Labels, _: i64) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn cancelled_requests(&self, _: &Labels) {}
    fn deadline_exceeded_requests(&self, _: &Labels) {}
    fn bytes_sent(&self, _: &Labels, _: u64) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn bytes_received(&self, _: &Labels, _: u64) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn messages_sent(&self, _: &Labels, _: u64) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn messages_received(&self, _: &Labels, _: u64) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn decode_failures(&self, _: &Labels) {}
    fn encode_failures(&self, _: &Labels) {}
    fn connection_errors(&self, _: &Labels) {}
    fn stream_resets(&self, _: &Labels) {}
}

impl Logger for Counting {
    fn log(&self, _: &LogEvent) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

struct Impl;

#[async_trait]
impl Bench for Impl {
    async fn unary(&self, _ctx: &RpcContext, request: Small) -> Result<Small, RpcError> {
        Ok(request)
    }
    async fn echo(&self, _ctx: &RpcContext, request: Blob) -> Result<Blob, RpcError> {
        Ok(request)
    }
    async fn stream(
        &self,
        _ctx: &RpcContext,
        _request: Small,
    ) -> Result<BoxStream<'static, Result<Small, RpcError>>, RpcError> {
        Err(RpcError::unimplemented("unused").finish())
    }
    async fn slow(&self, _ctx: &RpcContext, request: Small) -> Result<Small, RpcError> {
        Ok(request)
    }
}

fn observed(c: &mut Criterion) {
    let backend: &'static Counting = Box::leak(Box::new(Counting(AtomicU64::new(0))));
    set_global_metrics(backend);
    set_global_logger(backend);

    let rt = Runtime::new().unwrap();
    let (srv, rx) = InProcessServer::bind(1024);
    rt.spawn(
        std::sync::Arc::new(Server::new().add_service(BenchServer::new(Impl))).serve_in_process(rx),
    );
    let client = BenchClient::new(Channel::new(srv.transport()));
    let ctx = RpcContext::new();
    let request = Small {
        id: 7,
        name: "benchmark".into(),
        flag: true,
        ..Default::default()
    };

    let mut g = c.benchmark_group("rpc_observed");
    g.sample_size(30);
    g.measurement_time(Duration::from_secs(3));
    g.bench_function("unary/in_process_with_metrics_and_logs", |b| {
        b.to_async(&rt)
            .iter(|| async { client.unary(&ctx, &request).await.unwrap() })
    });
    g.finish();
    assert!(backend.0.load(Ordering::Relaxed) > 0);
}

criterion_group!(benches, observed);
criterion_main!(benches);

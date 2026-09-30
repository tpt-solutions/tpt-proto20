//! Metrics, structured logs and trace propagation across a real call path.
//!
//! The observability hooks are process-global and set-once, so everything
//! lives in a single test in its own binary.

use futures::stream::BoxStream;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tpt20_core::DecodeError;
use tpt20_observability::{
    set_global_logger, set_global_metrics, Labels, LogEvent, Logger, Metrics,
};
use tpt20_rpc::trace::TraceContext;
use tpt20_rpc::{async_trait, Channel, RpcContext, RpcError, Server, ServerCall, Service, Status};
use tpt20_transport::InProcessServer;

#[derive(Default)]
struct Recorder {
    counts: Mutex<HashMap<String, i64>>,
    logs: Mutex<Vec<LogEvent>>,
}

impl Recorder {
    fn add(&self, name: &str, l: &Labels, n: i64) {
        let key = format!(
            "{name}|{}|{}|{}|{}|{}",
            l.transport, l.service, l.method, l.streaming_type, l.status
        );
        *self.counts.lock().unwrap().entry(key).or_default() += n;
    }

    /// Sum of `name` over all label sets whose key contains every fragment.
    fn sum(&self, name: &str, fragments: &[&str]) -> i64 {
        self.counts
            .lock()
            .unwrap()
            .iter()
            .filter(|(k, _)| {
                let fields: Vec<&str> = k.split('|').collect();
                fields[0] == name && fragments.iter().all(|f| fields.contains(f))
            })
            .map(|(_, v)| *v)
            .sum()
    }
}

impl Metrics for Recorder {
    fn requests_started(&self, l: &Labels) {
        self.add("started", l, 1)
    }
    fn requests_completed(&self, l: &Labels) {
        self.add("completed", l, 1)
    }
    fn request_duration(&self, l: &Labels, d: Duration) {
        assert!(d < Duration::from_secs(60));
        self.add("duration_samples", l, 1)
    }
    fn active_streams(&self, l: &Labels, delta: i64) {
        // The streaming label is part of the key only for balance checks per
        // side, so strip it.
        let mut l = l.clone();
        l.streaming_type.clear();
        self.add("active", &l, delta)
    }
    fn cancelled_requests(&self, l: &Labels) {
        self.add("cancelled", l, 1)
    }
    fn deadline_exceeded_requests(&self, l: &Labels) {
        self.add("deadline", l, 1)
    }
    fn bytes_sent(&self, l: &Labels, b: u64) {
        self.add("bytes_sent", l, b as i64)
    }
    fn bytes_received(&self, l: &Labels, b: u64) {
        self.add("bytes_received", l, b as i64)
    }
    fn messages_sent(&self, l: &Labels, n: u64) {
        self.add("msgs_sent", l, n as i64)
    }
    fn messages_received(&self, l: &Labels, n: u64) {
        self.add("msgs_received", l, n as i64)
    }
    fn decode_failures(&self, l: &Labels) {
        self.add("decode_failures", l, 1)
    }
    fn encode_failures(&self, l: &Labels) {
        self.add("encode_failures", l, 1)
    }
    fn connection_errors(&self, l: &Labels) {
        self.add("connection_errors", l, 1)
    }
    fn stream_resets(&self, l: &Labels) {
        self.add("stream_resets", l, 1)
    }
}

impl Logger for Recorder {
    fn log(&self, event: &LogEvent) {
        self.logs.lock().unwrap().push(event.clone());
    }
}

fn dec(b: &[u8]) -> Result<Vec<u8>, DecodeError> {
    Ok(b.to_vec())
}
#[allow(clippy::ptr_arg)]
fn enc(v: &Vec<u8>) -> Vec<u8> {
    v.clone()
}
fn strict_dec(b: &[u8]) -> Result<Vec<u8>, DecodeError> {
    if b == b"bad" {
        Err(DecodeError::Truncated)
    } else {
        Ok(b.to_vec())
    }
}

struct Svc;

#[async_trait]
impl Service for Svc {
    fn name(&self) -> &'static str {
        "obs.Svc"
    }

    async fn handle(&self, method: &str, call: ServerCall) {
        match method {
            "Upper" => {
                call.unary(dec, enc, |_c, r| async move { Ok(r.to_ascii_uppercase()) })
                    .await
            }
            "Strict" => {
                call.unary(strict_dec, enc, |_c, r| async move { Ok(r) })
                    .await
            }
            "Trace" => {
                call.unary(dec, enc, |ctx, _r| async move {
                    Ok(ctx.trace().trace_id.clone().into_bytes())
                })
                .await
            }
            "Fail" => {
                call.unary(dec, enc, |_c, _r: Vec<u8>| async move {
                    Err::<Vec<u8>, _>(RpcError::not_found("nope").finish())
                })
                .await
            }
            "Slow" => {
                call.unary(dec, enc, |_c, _r: Vec<u8>| async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    Ok(vec![])
                })
                .await
            }
            "Count" => {
                call.server_streaming(dec, enc, |_c, r: Vec<u8>| async move {
                    let s: BoxStream<'static, Result<Vec<u8>, RpcError>> =
                        futures::stream::iter((0..r[0]).map(|i| Ok(vec![i]))).boxed();
                    Ok(s)
                })
                .await
            }
            "Sum" => {
                call.client_streaming(dec, enc, |_c, mut reqs| async move {
                    let mut n = 0u8;
                    while let Some(r) = reqs.next().await {
                        n += r?.len() as u8;
                    }
                    Ok(vec![n])
                })
                .await
            }
            other => {
                call.finish(Err(RpcError::unimplemented(other.to_string()).finish()))
                    .await
            }
        }
    }
}

async fn eventually(rec: &Recorder, what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..1000 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut dump: Vec<_> = rec
        .counts
        .lock()
        .unwrap()
        .iter()
        .map(|(k, v)| format!("{k} = {v}"))
        .collect();
    dump.sort();
    panic!("timed out waiting for: {what}\n{}", dump.join("\n"));
}

#[tokio::test]
async fn calls_are_measured_logged_and_traced() {
    let rec: &'static Recorder = Box::leak(Box::new(Recorder::default()));
    set_global_metrics(rec);
    set_global_logger(rec);

    let (srv, rx) = InProcessServer::bind(16);
    tokio::spawn(Arc::new(Server::new().add_service(Svc)).serve_in_process(rx));
    let ch = Channel::new(srv.transport());
    let ctx = RpcContext::new();

    // ---- unary success -----------------------------------------------------
    let r = ch
        .unary("obs.Svc/Upper", &ctx, b"abc".to_vec(), dec)
        .await
        .unwrap();
    assert_eq!(r, b"ABC");
    eventually(rec, "server completion", || {
        rec.sum("completed", &["server", "Upper", "OK"]) == 1
    })
    .await;
    for side in ["client", "server"] {
        assert_eq!(
            rec.sum("started", &[side, "obs.Svc", "Upper", "unary"]),
            1,
            "{side}"
        );
        assert_eq!(
            rec.sum("completed", &[side, "Upper", "unary", "OK"]),
            1,
            "{side}"
        );
        assert_eq!(
            rec.sum("duration_samples", &[side, "Upper", "OK"]),
            1,
            "{side}"
        );
        assert_eq!(rec.sum("msgs_sent", &[side, "Upper"]), 1, "{side}");
        assert_eq!(rec.sum("msgs_received", &[side, "Upper"]), 1, "{side}");
        assert_eq!(rec.sum("bytes_sent", &[side, "Upper"]), 3, "{side}");
        assert_eq!(rec.sum("bytes_received", &[side, "Upper"]), 3, "{side}");
    }

    // ---- error statuses ------------------------------------------------------
    let e = ch
        .unary("obs.Svc/Fail", &ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::NotFound);
    eventually(rec, "server NOT_FOUND", || {
        rec.sum("completed", &["server", "Fail", "NOT_FOUND"]) == 1
    })
    .await;
    assert_eq!(rec.sum("completed", &["client", "Fail", "NOT_FOUND"]), 1);

    // Unknown method: counted, with an empty streaming label, as UNIMPLEMENTED.
    let e = ch
        .unary("obs.Svc/Nope", &ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::Unimplemented);
    eventually(rec, "unimplemented", || {
        rec.sum("completed", &["server", "Nope", "UNIMPLEMENTED"]) == 1
    })
    .await;
    assert_eq!(rec.sum("started", &["server", "Nope"]), 1);

    // ---- decode failures -------------------------------------------------------
    let e = ch
        .unary("obs.Svc/Strict", &ctx, b"bad".to_vec(), dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::InvalidArgument);
    eventually(rec, "decode failure", || {
        rec.sum("decode_failures", &["server", "Strict"]) == 1
    })
    .await;

    // ---- deadline and cancellation ---------------------------------------------
    let short = RpcContext::new().with_timeout(Duration::from_millis(100));
    let e = ch
        .unary("obs.Svc/Slow", &short, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::DeadlineExceeded);
    assert_eq!(rec.sum("deadline", &["client", "Slow"]), 1);
    eventually(rec, "server deadline", || {
        rec.sum("deadline", &["server", "Slow"]) == 1
    })
    .await;

    let cancel_ctx = RpcContext::new();
    let token = cancel_ctx.cancellation().clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token.cancel();
    });
    let e = ch
        .unary("obs.Svc/Slow", &cancel_ctx, vec![], dec)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Status::Cancelled);
    assert_eq!(rec.sum("cancelled", &["client", "Slow"]), 1);

    // ---- streaming shapes ----------------------------------------------------------
    let s = ch
        .server_streaming("obs.Svc/Count", &ctx, vec![4], dec)
        .await
        .unwrap();
    assert_eq!(s.count().await, 4);
    eventually(rec, "server stream done", || {
        rec.sum("completed", &["server", "Count", "server_streaming", "OK"]) == 1
    })
    .await;
    assert_eq!(rec.sum("msgs_received", &["client", "Count"]), 4);
    assert_eq!(rec.sum("msgs_sent", &["server", "Count"]), 4);

    let reqs = futures::stream::iter(vec![vec![1u8, 2], vec![3]]).boxed();
    let r = ch
        .client_streaming("obs.Svc/Sum", &ctx, reqs, dec)
        .await
        .unwrap();
    assert_eq!(r, vec![3]);
    eventually(rec, "client stream done", || {
        rec.sum("completed", &["server", "Sum", "client_streaming", "OK"]) == 1
    })
    .await;
    assert_eq!(rec.sum("msgs_sent", &["client", "Sum"]), 2);
    assert_eq!(rec.sum("msgs_received", &["server", "Sum"]), 2);

    // A response stream dropped early is reported as cancelled, not lost.
    let mut s = ch
        .server_streaming("obs.Svc/Count", &ctx, vec![200], dec)
        .await
        .unwrap();
    let _ = s.next().await;
    drop(s);
    assert_eq!(rec.sum("cancelled", &["client", "Count"]), 1);

    // ---- trace context propagation ------------------------------------------------
    let traced = RpcContext::new().with_trace(TraceContext::new(
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "00f067aa0ba902b7",
        1,
    ));
    let seen = ch
        .unary("obs.Svc/Trace", &traced, vec![], dec)
        .await
        .unwrap();
    assert_eq!(seen, b"4bf92f3577b34da6a3ce929d0e0e4736");

    // ---- balance + logs --------------------------------------------------------------
    eventually(rec, "all calls finished", || {
        rec.sum("active", &["client"]) == 0 && rec.sum("active", &["server"]) == 0
    })
    .await;
    assert_eq!(
        rec.sum("started", &["client"]),
        rec.sum("completed", &["client"])
    );
    assert_eq!(
        rec.sum("started", &["server"]),
        rec.sum("completed", &["server"])
    );

    let logs = rec.logs.lock().unwrap().clone();
    let trace_logs: Vec<_> = logs
        .iter()
        .filter(|l| l.method.as_deref() == Some("Trace"))
        .collect();
    assert_eq!(trace_logs.len(), 2, "client + server log line");
    for l in &trace_logs {
        assert_eq!(l.service.as_deref(), Some("obs.Svc"));
        assert_eq!(l.status.as_deref(), Some("OK"));
        assert_eq!(
            l.request_id.as_deref(),
            Some("4bf92f3577b34da6a3ce929d0e0e4736")
        );
        assert!(l.deadline.is_some());
    }
    let fail = logs
        .iter()
        .find(|l| l.method.as_deref() == Some("Fail"))
        .unwrap();
    assert_eq!(fail.status.as_deref(), Some("NOT_FOUND"));
    let cancelled = logs
        .iter()
        .find(|l| l.status.as_deref() == Some("CANCELLED") && l.method.as_deref() == Some("Slow"))
        .unwrap();
    assert!(cancelled.cancellation_reason.is_some());
    assert!(logs.iter().all(|l| l.request_id.is_some()));
}

//! Observability wiring (spec §19): per-call metrics and structured logs for
//! both sides of an RPC, delivered through the `tpt20-observability` global
//! hooks. When no metrics backend and no logger are registered a
//! [`CallObserver`] is inert and costs a couple of atomic loads per call.
//!
//! Labels: `service`, `method`, `streaming_type` (`unary`, `server_streaming`,
//! `client_streaming`, `bidi_streaming`), `status` (set on completion) and
//! `transport` (`client` / `server`: the RPC layer does not know the concrete
//! transport, so it reports which side observed the call).

use crate::error::RpcError;
use crate::status::Status;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tpt20_observability::{emit_log, global_logger, global_metrics, Labels, LogEvent, Metrics};
use tpt20_transport::TransportError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Client,
    Server,
}

impl Side {
    fn label(self) -> &'static str {
        match self {
            Side::Client => "client",
            Side::Server => "server",
        }
    }
}

/// Extra facts included in the completion log line.
#[derive(Default, Clone)]
pub(crate) struct CallInfo {
    pub(crate) request_id: Option<String>,
    pub(crate) peer: Option<String>,
    pub(crate) deadline: Option<Duration>,
}

struct Shared {
    metrics: Option<&'static dyn Metrics>,
    log: bool,
    labels: Labels,
    service: String,
    method: String,
    start: Instant,
    finished: AtomicBool,
    started: AtomicBool,
    streaming: OnceLock<&'static str>,
    info: CallInfo,
}

/// A cheap, cloneable handle recording one call's telemetry. The call is
/// reported as cancelled if the last handle is dropped before
/// [`finish`](Self::finish) (for example a response stream dropped early).
#[derive(Clone)]
pub(crate) struct CallObserver(Option<Arc<Shared>>);

static REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

impl CallObserver {
    /// Starts observing the call to `path` (`pkg.Service/Method`).
    pub(crate) fn start(
        side: Side,
        path: &str,
        streaming: Option<&'static str>,
        mut info: CallInfo,
    ) -> CallObserver {
        let metrics = global_metrics();
        let log = global_logger().is_some();
        if metrics.is_none() && !log {
            return CallObserver(None);
        }
        let (service, method) = path.split_once('/').unwrap_or(("", path));
        if info.request_id.is_none() {
            info.request_id = Some(format!(
                "req-{}",
                REQUEST_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
        }
        let labels = Labels::new()
            .service(service)
            .method(method)
            .transport(side.label());
        let shared = Shared {
            metrics,
            log,
            labels,
            service: service.to_string(),
            method: method.to_string(),
            start: Instant::now(),
            finished: AtomicBool::new(false),
            started: AtomicBool::new(false),
            streaming: OnceLock::new(),
            info,
        };
        if let Some(s) = streaming {
            let _ = shared.streaming.set(s);
        }
        let obs = CallObserver(Some(Arc::new(shared)));
        if streaming.is_some() {
            if let Some(sh) = &obs.0 {
                sh.mark_started();
            }
        }
        obs
    }

    /// Sets the streaming-type label once it is known (server side).
    pub(crate) fn set_streaming(&self, streaming: &'static str) {
        if let Some(sh) = &self.0 {
            let _ = sh.streaming.set(streaming);
            sh.mark_started();
        }
    }

    pub(crate) fn message_sent(&self, bytes: usize) {
        if let Some(sh) = &self.0 {
            if let Some(m) = sh.metrics {
                let l = sh.labels_now("");
                m.messages_sent(&l, 1);
                m.bytes_sent(&l, bytes as u64);
            }
        }
    }

    pub(crate) fn message_received(&self, bytes: usize) {
        if let Some(sh) = &self.0 {
            if let Some(m) = sh.metrics {
                let l = sh.labels_now("");
                m.messages_received(&l, 1);
                m.bytes_received(&l, bytes as u64);
            }
        }
    }

    pub(crate) fn decode_failure(&self) {
        if let Some(sh) = &self.0 {
            if let Some(m) = sh.metrics {
                m.decode_failures(&sh.labels_now(""));
            }
        }
    }

    pub(crate) fn transport_error(&self, e: &TransportError) {
        if let Some(sh) = &self.0 {
            if let Some(m) = sh.metrics {
                let l = sh.labels_now("");
                match e {
                    TransportError::StreamReset => m.stream_resets(&l),
                    TransportError::ConnectionClosed
                    | TransportError::GoAway(_)
                    | TransportError::Io(_)
                    | TransportError::Tls(_) => m.connection_errors(&l),
                    _ => {}
                }
            }
        }
    }

    /// Records the outcome of a call that yields a value.
    pub(crate) fn finish_result<T>(&self, result: &Result<T, RpcError>) {
        match result {
            Ok(_) => self.finish(Status::Ok, ""),
            Err(e) => self.finish(e.status(), e.message()),
        }
    }

    /// Records completion (first call wins).
    pub(crate) fn finish(&self, status: Status, message: &str) {
        if let Some(sh) = &self.0 {
            sh.finish(status, message);
        }
    }
}

impl Shared {
    /// Counts the call as started. On the server the streaming type is only
    /// known once dispatch picks a method, so this is deferred until then (or
    /// until the call finishes) to keep one consistent label set per call.
    fn mark_started(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(m) = self.metrics {
            let l = self.labels_now("");
            m.requests_started(&l);
            m.active_streams(&l, 1);
        }
    }

    fn labels_now(&self, status: &str) -> Labels {
        let mut l = self.labels.clone();
        l.streaming_type = self.streaming.get().copied().unwrap_or("").to_string();
        l.status = status.to_string();
        l
    }

    fn finish(&self, status: Status, message: &str) {
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        self.mark_started();
        let elapsed = self.start.elapsed();
        if let Some(m) = self.metrics {
            let open = self.labels_now("");
            let done = self.labels_now(status.as_str());
            m.active_streams(&open, -1);
            m.request_duration(&done, elapsed);
            m.requests_completed(&done);
            match status {
                Status::Cancelled => m.cancelled_requests(&done),
                Status::DeadlineExceeded => m.deadline_exceeded_requests(&done),
                _ => {}
            }
        }
        if self.log {
            let mut event = LogEvent::new()
                .service(&self.service)
                .method(&self.method)
                .status(status.as_str());
            if let Some(id) = &self.info.request_id {
                event = event.request_id(id);
            }
            if let Some(peer) = &self.info.peer {
                event = event.peer_info(peer);
            }
            if let Some(d) = self.info.deadline {
                event = event.deadline(format!("{}ms", d.as_millis()));
            }
            if status == Status::Cancelled {
                event = event.cancellation_reason(if message.is_empty() {
                    "cancelled"
                } else {
                    message
                });
            }
            emit_log(event);
        }
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        // The call never reported an outcome: its handle went away first.
        self.finish(Status::Cancelled, "call abandoned before completion");
    }
}

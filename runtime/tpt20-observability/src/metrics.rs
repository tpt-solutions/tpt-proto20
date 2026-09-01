//! Metrics API for tpt20 (spec §19.1).
//!
//! This module defines the [`Metrics`] trait and the label set used across all
//! RPC and codec metrics. Implementations are expected to map these to the
//! monitoring system in use (Prometheus, OpenTelemetry, etc.).

use std::time::Duration;

/// Metric label dimensions.
///
/// All metrics share this label set where applicable. Fields that are not
/// relevant for a given metric may be left empty.
#[derive(Debug, Clone, Default)]
pub struct Labels {
    /// Service name, e.g. `"user.v1.UserService"`.
    pub service: String,
    /// Method name, e.g. `"GetUser"`.
    pub method: String,
    /// RPC status, e.g. `"OK"`, `"NOT_FOUND"`, `"DEADLINE_EXCEEDED"`.
    pub status: String,
    /// Streaming type: `"unary"`, `"server_streaming"`, `"client_streaming"`,
    /// `"bidi_streaming"`.
    pub streaming_type: String,
    /// Transport name, e.g. `"tcp"`, `"uds"`.
    pub transport: String,
}

impl Labels {
    /// Creates an empty label set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the service label.
    pub fn service(mut self, value: impl Into<String>) -> Self {
        self.service = value.into();
        self
    }

    /// Sets the method label.
    pub fn method(mut self, value: impl Into<String>) -> Self {
        self.method = value.into();
        self
    }

    /// Sets the status label.
    pub fn status(mut self, value: impl Into<String>) -> Self {
        self.status = value.into();
        self
    }

    /// Sets the streaming_type label.
    pub fn streaming_type(mut self, value: impl Into<String>) -> Self {
        self.streaming_type = value.into();
        self
    }

    /// Sets the transport label.
    pub fn transport(mut self, value: impl Into<String>) -> Self {
        self.transport = value.into();
        self
    }
}

/// Metrics backend.
///
/// Implementations record telemetry data. The runtime calls these methods at
/// well-defined instrumentation points. All methods must be thread-safe.
pub trait Metrics: Send + Sync {
    /// A new RPC request was initiated.
    fn requests_started(&self, labels: &Labels);

    /// An RPC request completed (success or terminal failure).
    fn requests_completed(&self, labels: &Labels);

    /// Elapsed time for a completed request.
    fn request_duration(&self, labels: &Labels, duration: Duration);

    /// Change in the number of active streams. `delta` is positive on stream
    /// creation and negative on stream termination.
    fn active_streams(&self, labels: &Labels, delta: i64);

    /// A request was cancelled by the caller.
    fn cancelled_requests(&self, labels: &Labels);

    /// A request exceeded its deadline.
    fn deadline_exceeded_requests(&self, labels: &Labels);

    /// Bytes sent on the wire (encoded payload).
    fn bytes_sent(&self, labels: &Labels, bytes: u64);

    /// Bytes received from the wire (decoded payload).
    fn bytes_received(&self, labels: &Labels, bytes: u64);

    /// Messages sent on a stream.
    fn messages_sent(&self, labels: &Labels, count: u64);

    /// Messages received from a stream.
    fn messages_received(&self, labels: &Labels, count: u64);

    /// A message failed to decode.
    fn decode_failures(&self, labels: &Labels);

    /// A message failed to encode.
    fn encode_failures(&self, labels: &Labels);

    /// A transport-level connection error occurred.
    fn connection_errors(&self, labels: &Labels);

    /// A stream was reset (RST_STREAM or equivalent).
    fn stream_resets(&self, labels: &Labels);
}

/// A [`Metrics`] implementation that discards all data.
///
/// This is the default when no metrics backend is configured. It has zero
/// allocation and zero synchronization overhead.
#[derive(Debug, Clone, Default)]
pub struct NoopMetrics;

impl Metrics for NoopMetrics {
    fn requests_started(&self, _labels: &Labels) {}
    fn requests_completed(&self, _labels: &Labels) {}
    fn request_duration(&self, _labels: &Labels, _duration: Duration) {}
    fn active_streams(&self, _labels: &Labels, _delta: i64) {}
    fn cancelled_requests(&self, _labels: &Labels) {}
    fn deadline_exceeded_requests(&self, _labels: &Labels) {}
    fn bytes_sent(&self, _labels: &Labels, _bytes: u64) {}
    fn bytes_received(&self, _labels: &Labels, _bytes: u64) {}
    fn messages_sent(&self, _labels: &Labels, _count: u64) {}
    fn messages_received(&self, _labels: &Labels, _count: u64) {}
    fn decode_failures(&self, _labels: &Labels) {}
    fn encode_failures(&self, _labels: &Labels) {}
    fn connection_errors(&self, _labels: &Labels) {}
    fn stream_resets(&self, _labels: &Labels) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_builder() {
        let labels = Labels::new()
            .service("user.v1.UserService")
            .method("GetUser")
            .status("OK")
            .streaming_type("unary")
            .transport("tcp");

        assert_eq!(labels.service, "user.v1.UserService");
        assert_eq!(labels.method, "GetUser");
        assert_eq!(labels.status, "OK");
        assert_eq!(labels.streaming_type, "unary");
        assert_eq!(labels.transport, "tcp");
    }

    #[test]
    fn noop_metrics_are_silent() {
        let metrics = NoopMetrics;
        let labels = Labels::new();
        metrics.requests_started(&labels);
        metrics.requests_completed(&labels);
        metrics.request_duration(&labels, Duration::from_millis(1));
        metrics.active_streams(&labels, 1);
        metrics.active_streams(&labels, -1);
        metrics.cancelled_requests(&labels);
        metrics.deadline_exceeded_requests(&labels);
        metrics.bytes_sent(&labels, 100);
        metrics.bytes_received(&labels, 200);
        metrics.messages_sent(&labels, 1);
        metrics.messages_received(&labels, 1);
        metrics.decode_failures(&labels);
        metrics.encode_failures(&labels);
        metrics.connection_errors(&labels);
        metrics.stream_resets(&labels);
    }
}

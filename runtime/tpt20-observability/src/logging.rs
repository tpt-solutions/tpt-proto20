//! Structured logging types for tpt20 (spec §19.3).
//!
//! [`LogEvent`] captures the fields that every structured log line should
//! contain. Backends may serialize these to JSON, text, or any other format.

/// A structured log event emitted by the RPC runtime.
///
/// All fields are optional; backends should emit only the fields that are
/// populated.
#[derive(Debug, Clone, Default)]
pub struct LogEvent {
    /// Unique request identifier (e.g. UUID or trace ID).
    pub request_id: Option<String>,
    /// Service name, e.g. `"user.v1.UserService"`.
    pub service: Option<String>,
    /// Method name, e.g. `"GetUser"`.
    pub method: Option<String>,
    /// Final RPC status, e.g. `"OK"`, `"NOT_FOUND"`.
    pub status: Option<String>,
    /// Deadline as an ISO 8601 timestamp or duration string.
    pub deadline: Option<String>,
    /// Cancellation reason when the request was cancelled.
    pub cancellation_reason: Option<String>,
    /// Peer address where privacy policy allows disclosure.
    pub peer_info: Option<String>,
    /// Schema fingerprint where useful for schema-aware debugging.
    pub schema_fingerprint: Option<String>,
}

impl LogEvent {
    /// Creates an empty log event.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the request ID.
    pub fn request_id(mut self, value: impl Into<String>) -> Self {
        self.request_id = Some(value.into());
        self
    }

    /// Sets the service name.
    pub fn service(mut self, value: impl Into<String>) -> Self {
        self.service = Some(value.into());
        self
    }

    /// Sets the method name.
    pub fn method(mut self, value: impl Into<String>) -> Self {
        self.method = Some(value.into());
        self
    }

    /// Sets the RPC status.
    pub fn status(mut self, value: impl Into<String>) -> Self {
        self.status = Some(value.into());
        self
    }

    /// Sets the deadline.
    pub fn deadline(mut self, value: impl Into<String>) -> Self {
        self.deadline = Some(value.into());
        self
    }

    /// Sets the cancellation reason.
    pub fn cancellation_reason(mut self, value: impl Into<String>) -> Self {
        self.cancellation_reason = Some(value.into());
        self
    }

    /// Sets the peer info.
    pub fn peer_info(mut self, value: impl Into<String>) -> Self {
        self.peer_info = Some(value.into());
        self
    }

    /// Sets the schema fingerprint.
    pub fn schema_fingerprint(mut self, value: impl Into<String>) -> Self {
        self.schema_fingerprint = Some(value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_event_builder() {
        let event = LogEvent::new()
            .request_id("req-123")
            .service("user.v1.UserService")
            .method("GetUser")
            .status("OK")
            .deadline("2026-09-01T01:00:00Z")
            .cancellation_reason("client_cancelled")
            .peer_info("192.168.1.1:443")
            .schema_fingerprint("abcd1234");

        assert_eq!(event.request_id, Some("req-123".to_string()));
        assert_eq!(event.service, Some("user.v1.UserService".to_string()));
        assert_eq!(event.method, Some("GetUser".to_string()));
        assert_eq!(event.status, Some("OK".to_string()));
        assert_eq!(event.deadline, Some("2026-09-01T01:00:00Z".to_string()));
        assert_eq!(
            event.cancellation_reason,
            Some("client_cancelled".to_string())
        );
        assert_eq!(event.peer_info, Some("192.168.1.1:443".to_string()));
        assert_eq!(event.schema_fingerprint, Some("abcd1234".to_string()));
    }
}

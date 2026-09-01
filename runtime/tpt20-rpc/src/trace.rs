//! Trace context for distributed tracing (spec §16.1).

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub trace_flags: u8,
    pub trace_state: String,
}

impl TraceContext {
    pub fn new(trace_id: impl Into<String>, span_id: impl Into<String>, trace_flags: u8) -> Self {
        Self { trace_id: trace_id.into(), span_id: span_id.into(), trace_flags, trace_state: String::new() }
    }
    pub fn is_sampled(&self) -> bool { self.trace_flags & 0x01 != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_context_helpers() {
        let ctx = TraceContext::new("trace-123", "span-456", 0x01);
        assert!(ctx.is_sampled());
        assert_eq!(ctx.trace_id, "trace-123");
    }
}

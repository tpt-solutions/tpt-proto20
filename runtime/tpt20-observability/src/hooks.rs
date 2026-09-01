//! Optional global hooks for observability integration.
//!
//! This module provides a global hook registry that the RPC runtime and core
//! codec can call without depending on a concrete observability backend.
//!
//! When no hooks are registered, calls are no-ops.

use std::sync::OnceLock;

use crate::logging::LogEvent;
use crate::metrics::{Metrics, NoopMetrics};

static GLOBAL_METRICS: OnceLock<&'static dyn Metrics> = OnceLock::new();

/// Sets the global [`Metrics`] implementation.
///
/// This may be called at most once. Subsequent calls are silently ignored.
/// Pass [`crate::NoopMetrics`] to disable metrics collection.
pub fn set_global_metrics(metrics: &'static dyn Metrics) {
    let _ = GLOBAL_METRICS.set(metrics);
}

/// Returns the global [`Metrics`] implementation, if one was registered.
pub fn global_metrics() -> Option<&'static dyn Metrics> {
    GLOBAL_METRICS.get().copied()
}

/// Returns the global [`Metrics`] implementation, or a no-op fallback.
pub fn global_metrics_or_noop() -> &'static dyn Metrics {
    GLOBAL_METRICS.get().copied().unwrap_or(&NoopMetrics)
}

/// A combined hook set that the runtime can query once.
///
/// This struct holds references to the global observability backends so that
/// hot paths need only a single lookup.
pub struct GlobalHooks {
    /// Metrics backend.
    pub metrics: &'static dyn Metrics,
}

impl GlobalHooks {
    /// Builds a [`GlobalHooks`] from the currently registered backends.
    pub fn current() -> Self {
        Self {
            metrics: global_metrics_or_noop(),
        }
    }
}

/// Emits a structured log event if a global logger is registered.
///
/// This is a convenience wrapper. Backends that want to observe log events
/// should implement their own logger interface; this function is a placeholder
/// that can be wired up when a concrete logging backend is available.
pub fn emit_log(_event: LogEvent) {}

#[cfg(test)]
mod tests {
    use crate::metrics::Labels;
    use super::*;

    #[test]
    fn global_hooks_returns_noop_when_unset() {
        let hooks = GlobalHooks::current();
        let labels = Labels::new();
        hooks.metrics.requests_started(&labels);
    }

    #[test]
    fn set_and_get_global_metrics() {
        set_global_metrics(&NoopMetrics);
        assert!(global_metrics().is_some());
        // Reset for subsequent tests
        set_global_metrics(&NoopMetrics);
    }
}

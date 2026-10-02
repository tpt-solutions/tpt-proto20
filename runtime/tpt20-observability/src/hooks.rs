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

/// A sink for structured [`LogEvent`]s (spec §19.3).
pub trait Logger: Send + Sync {
    /// Receives one event. Must not block the calling RPC task for long.
    fn log(&self, event: &LogEvent);
}

static GLOBAL_LOGGER: OnceLock<&'static dyn Logger> = OnceLock::new();

/// Sets the global [`Logger`]. May be called at most once; later calls are
/// ignored.
pub fn set_global_logger(logger: &'static dyn Logger) {
    let _ = GLOBAL_LOGGER.set(logger);
}

/// Returns the global [`Logger`], if one was registered.
pub fn global_logger() -> Option<&'static dyn Logger> {
    GLOBAL_LOGGER.get().copied()
}

/// Delivers `event` to the global logger; a no-op when none is registered.
pub fn emit_log(event: LogEvent) {
    if let Some(logger) = global_logger() {
        logger.log(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Labels;

    #[test]
    fn global_hooks_returns_noop_when_unset() {
        let hooks = GlobalHooks::current();
        let labels = Labels::new();
        hooks.metrics.requests_started(&labels);
    }

    #[test]
    fn logger_receives_events_once_registered() {
        use std::sync::Mutex;
        struct Capture(Mutex<Vec<String>>);
        impl Logger for Capture {
            fn log(&self, e: &LogEvent) {
                self.0
                    .lock()
                    .unwrap()
                    .push(e.status.clone().unwrap_or_default());
            }
        }
        // No logger yet: a no-op.
        emit_log(LogEvent::new().status("dropped"));
        let capture: &'static Capture = Box::leak(Box::new(Capture(Mutex::new(Vec::new()))));
        set_global_logger(capture);
        emit_log(LogEvent::new().status("OK"));
        assert_eq!(*capture.0.lock().unwrap(), vec!["OK".to_string()]);
    }

    #[test]
    fn set_and_get_global_metrics() {
        set_global_metrics(&NoopMetrics);
        assert!(global_metrics().is_some());
        // Reset for subsequent tests
        set_global_metrics(&NoopMetrics);
    }
}

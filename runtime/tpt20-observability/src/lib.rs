//! `tpt20-observability`: metrics, tracing, and structured logging for tpt20 (spec §19).
//!
//! This crate defines the observability API used by the RPC runtime and transport
//! layers. It is intentionally free of concrete backend implementations: consumers
//! provide a [`Metrics`] implementation and the runtime calls into it. A no-op
//! implementation ([`NoopMetrics`]) is provided for builds that do not require
//! telemetry collection.
//!
//! ## Metrics
//!
//! Counters, gauges, and histograms are exposed through the [`Metrics`] trait.
//! Label dimensions are defined in [`metrics::Labels`].
//!
//! ## Tracing
//!
//! Span attribute names are defined as constants in the [`tracing`] module. These
//! match the OpenTelemetry semantic conventions for RPC (`rpc.system`, `rpc.service`,
//! `rpc.method`, `rpc.status`, `rpc.schema_fingerprint`).
//!
//! ## Structured logging
//!
//! Log events are represented as [`logging::LogEvent`] with typed fields. Backends
//! may serialize these to JSON or any other structured format.
//!
//! ## CLI schema-aware debugging
//!
//! The CLI `tpt20 decode --schema ... --message ...` command is deferred to Phase 16.
//! This crate does not contain CLI argument parsing.

pub mod hooks;
pub mod logging;
pub mod metrics;
pub mod tracing;

pub use hooks::{global_metrics, set_global_metrics, GlobalHooks};
pub use logging::LogEvent;
pub use metrics::{Labels, Metrics, NoopMetrics};
pub use tracing::{
    RPC_SCHEMA_FINGERPRINT, RPC_SERVICE, RPC_STATUS, RPC_METHOD, RPC_SYSTEM, RPC_SYSTEM_VALUE,
};

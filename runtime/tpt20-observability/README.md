# tpt20-observability

Observability primitives for tpt20: metrics, tracing, and structured logging (spec §19).

## Overview

This crate defines the observability API used by the RPC runtime and transport layers. It is intentionally free of concrete backend implementations: consumers provide a [`Metrics`] implementation and the runtime calls into it. A no-op implementation ([`NoopMetrics`]) is provided for builds that do not require telemetry collection.

## Metrics

Counters, gauges, and histograms are exposed through the [`Metrics`] trait. Label dimensions are defined in [`metrics::Labels`].

## Tracing

Span attribute names are defined as constants in the [`tracing`] module. These match the OpenTelemetry semantic conventions for RPC (`rpc.system`, `rpc.service`, `rpc.method`, `rpc.status`, `rpc.schema_fingerprint`).

## Structured logging

Log events are represented as [`logging::LogEvent`] with typed fields. Backends may serialize these to JSON or any other structured format.

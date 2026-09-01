# tpt20-transport

Transport layer for tpt20 RPC (spec §17, §18.6).

## Overview

This crate provides:

- Message framing: 1-byte flags + 4-byte big-endian length + N-byte payload
- Transport traits (transport-agnostic RPC interface)
- In-process transport (for tests, embedded, local dev, benchmarking, fuzzing)
- HTTP/2 transport (required production transport, feature-gated)
- QUIC/HTTP3 transport (optional, feature-gated)
- Custom stream transport extension point

## Feature flags

- `default` = `["in_process"]` — in-process transport is always available
- `http2` — HTTP/2 production transport (requires `h2`)
- `tls` — TLS with ALPN (requires `tokio-rustls` and `rustls-pemfile`)
- `quic` — QUIC/HTTP3 transport (requires `quinn`)

## Usage

```rust
use tpt20_transport::{InProcessTransport, Transport};

let transport = InProcessTransport::new();
// transport implements the `Transport` trait
```

# tpt20-compat-grpc

gRPC compatibility adapter for tpt-proto20 (spec §10.3).

## Overview

This crate provides the translation layer between gRPC and tpt20:

- HTTP/2 framing compatible with gRPC
- Protobuf-compatible message payload support
- Status code mapping (tpt20 ↔ gRPC)
- Metadata mapping (tpt20 ↔ gRPC)
- Deadline mapping (tpt20 ↔ gRPC `grpc-timeout` header)
- Streaming mode mapping (unary / server-stream / client-stream / bidi)
- gRPC message framing (5-byte header with MSB compression flag)
- gRPC health-checking protocol support
- gRPC reflection support (where feasible)

## Feature flags

- `server` — gRPC-compatible HTTP/2 server
- `client` — gRPC-compatible HTTP/2 client
- `reflection` — gRPC reflection protocol support
- `full` — all features

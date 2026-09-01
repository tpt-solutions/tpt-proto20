# tpt20-rpc

Remote Procedure Call system for tpt20 (spec §16).

## Overview

This crate provides the foundational types for RPC communication:

- [`RpcContext`] — per-call context carrying deadline, cancellation, metadata, trace, peer, and extensions
- [`Status`] — standard RPC status codes
- [`RpcError`] — rich errors with structured details and builder API
- Streaming abstractions: [`ServerStreamSink`], [`ClientStreamSource`], [`BidiStream`] — all backpressure-aware
- [`Metadata`] — case-normalized metadata with size limits
- [`Deadline`], [`CancellationToken`] — time and cancellation primitives
- [`RetryPolicy`] — configurable retry behavior
- [`Authenticator`], [`Authorizer`] — auth hooks

## Usage

```rust
use tpt20_rpc::{RpcContext, Status, RpcError};

fn handler(ctx: RpcContext, req: MyRequest) -> Result<MyResponse, RpcError> {
    if ctx.is_expired() {
        return Err(RpcError::new(Status::DeadlineExceeded, "deadline exceeded"));
    }
    Ok(MyResponse { /* ... */ })
}
```

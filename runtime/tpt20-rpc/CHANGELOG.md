# Changelog

All notable changes to `tpt20-rpc` are documented here.

## [Unreleased]

### Added
- Initial RPC type system (Phase 10, spec §16)
- `RpcContext` with deadline, cancellation, metadata, trace, peer, extensions
- `Status` codes: OK, CANCELLED, UNKNOWN, INVALID_ARGUMENT, DEADLINE_EXCEEDED, NOT_FOUND, ALREADY_EXISTS, PERMISSION_DENIED, RESOURCE_EXHAUSTED, FAILED_PRECONDITION, ABORTED, OUT_OF_RANGE, UNIMPLEMENTED, INTERNAL, UNAVAILABLE, DATA_LOSS, UNAUTHENTICATED
- `RpcError` with rich details and builder API
- `ServerStreamSink`, `ClientStreamSource`, `BidiStream` backpressure-aware streaming
- `Metadata` with case-normalized keys and size limits
- `Deadline` and `CancellationToken`
- `RetryPolicy`
- `Authenticator` / `Authorizer` auth hooks
- Compression support
- Authentication and authorization middleware
- TLS and mTLS support

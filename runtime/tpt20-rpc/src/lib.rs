//! `tpt20-rpc`: Remote Procedure Call system for tpt20 (spec §16).
//!
//! This crate provides the foundational types for RPC communication:
//!
//! - [`RpcContext`] — per-call context carrying deadline, cancellation,
//!   metadata, trace, peer, and extensions
//! - [`Status`] — standard RPC status codes
//! - [`RpcError`] — rich errors with structured details and builder API
//! - Streaming abstractions: [`ServerStreamSink`], [`ClientStreamSource`],
//!   [`BidiStream`] — all backpressure-aware
//! - [`Metadata`] — case-normalized metadata with size limits
//! - [`Deadline`], [`CancellationToken`] — time and cancellation primitives
//! - [`RetryPolicy`] — configurable retry behavior
//! - [`Authenticator`], [`Authorizer`] — auth hooks

pub mod auth;
pub mod cancellation;
pub mod client;
pub mod compression;
pub mod context;
pub mod deadline;
pub mod error;
pub mod extensions;
pub mod health;
pub mod metadata;
pub(crate) mod observe;
pub mod peer;
pub mod reflection;
pub mod retry;
pub mod server;
pub mod status;
pub mod stream;
pub mod trace;
pub mod wire;

pub use async_trait::async_trait;
pub use auth::{
    AclAuthorizer, AllowAllAuthorizer, AuthContext, AuthError, Authenticator, Authorizer,
    AuthzError, DenyAllAuthorizer, MetadataAuthenticator, RoleBasedAuthorizer, TokenAuthenticator,
};
pub use cancellation::CancellationToken;
pub use client::Channel;
pub use compression::CompressionAlgorithm;
pub use context::RpcContext;
pub use deadline::Deadline;
pub use error::{ReceiveError, RpcError, RpcErrorBuilder, SendError};
pub use extensions::Extensions;
pub use futures;
pub use futures::stream::BoxStream;
pub use metadata::{Metadata, MetadataError, MetadataKey, MetadataValue};
pub use peer::PeerInfo;
pub use retry::RetryPolicy;
pub use server::{ResponseSender, Server, ServerCall, Service};
pub use status::{Status, UnknownStatusCode};
pub use stream::{BidiStream, ClientStreamSource, ServerStreamSink, TrySink, TryStream};
pub use trace::TraceContext;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_context_creation() {
        let ctx = RpcContext::new();
        assert!(!ctx.is_expired());
        assert!(ctx.peer().is_none());
        assert!(ctx.extensions().is_empty());
    }
}

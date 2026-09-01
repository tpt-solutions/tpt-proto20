//! RPC context and related types (spec §16.1).

use std::time::Duration;

use crate::cancellation::CancellationToken;
use crate::deadline::Deadline;
use crate::extensions::Extensions;
use crate::metadata::Metadata;
use crate::peer::PeerInfo;
use crate::trace::TraceContext;

/// The context carried with every RPC call.
#[derive(Debug, Clone)]
pub struct RpcContext {
    deadline: Deadline,
    cancellation: CancellationToken,
    metadata: Metadata,
    trace: TraceContext,
    peer: Option<PeerInfo>,
    extensions: Extensions,
}

impl RpcContext {
    /// Creates a new RPC context with default values.
    pub fn new() -> Self {
        Self {
            deadline: Deadline::default(),
            cancellation: CancellationToken::new(),
            metadata: Metadata::with_default_limit(),
            trace: TraceContext::default(),
            peer: None,
            extensions: Extensions::new(),
        }
    }

    /// Sets the deadline for this context.
    pub fn with_deadline(mut self, deadline: Deadline) -> Self {
        self.deadline = deadline;
        self
    }

    /// Sets the cancellation token for this context.
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Sets the metadata for this context.
    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Sets the trace context for this call.
    pub fn with_trace(mut self, trace: TraceContext) -> Self {
        self.trace = trace;
        self
    }

    /// Sets the peer information.
    pub fn with_peer(mut self, peer: PeerInfo) -> Self {
        self.peer = Some(peer);
        self
    }

    /// Returns true if the deadline has expired.
    pub fn is_expired(&self) -> bool {
        self.deadline.is_expired()
    }

    /// Returns the remaining time until the deadline, or zero if expired.
    pub fn remaining_time(&self) -> Duration {
        self.deadline.remaining_time()
    }

    /// Returns a reference to the metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Returns a mutable reference to the metadata.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Returns a reference to the trace context.
    pub fn trace(&self) -> &TraceContext {
        &self.trace
    }

    /// Returns a reference to the cancellation token.
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Returns a reference to the deadline.
    pub fn deadline(&self) -> &Deadline {
        &self.deadline
    }

    /// Returns a reference to the peer info, if available.
    pub fn peer(&self) -> Option<&PeerInfo> {
        self.peer.as_ref()
    }

    /// Returns a mutable reference to the extensions map.
    pub fn extensions_mut(&mut self) -> &mut Extensions {
        &mut self.extensions
    }

    /// Returns a reference to the extensions map.
    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }
}

impl Default for RpcContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_builder() {
        let ctx = RpcContext::new()
            .with_deadline(Deadline::from_now(std::time::Duration::from_secs(5)))
            .with_trace(TraceContext::new("t1", "s1", 1))
            .with_peer(PeerInfo::new("127.0.0.1", 9090));

        assert!(!ctx.is_expired());
        assert_eq!(ctx.trace().trace_id, "t1");
        assert_eq!(ctx.peer().unwrap().addr, "127.0.0.1");
    }
}

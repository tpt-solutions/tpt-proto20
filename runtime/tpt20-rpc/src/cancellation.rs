//! Cancellation token for cooperative RPC cancellation (spec §16.1).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A token that signals cooperative cancellation of an RPC.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    inner: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a new, not-yet-cancelled token.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Creates a token that is already cancelled.
    pub fn cancelled() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Returns true if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }

    /// Requests cancellation. All clones of this token will observe the request.
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_not_cancelled() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancel_propagates_to_clones() {
        let token = CancellationToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
        assert!(token.is_cancelled());
    }

    #[test]
    fn pre_cancelled_token() {
        let token = CancellationToken::cancelled();
        assert!(token.is_cancelled());
    }
}

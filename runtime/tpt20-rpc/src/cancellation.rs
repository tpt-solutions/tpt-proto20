//! Cancellation token for cooperative RPC cancellation (spec §16.1).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

#[derive(Debug, Default)]
struct Inner {
    flag: AtomicBool,
    notify: Notify,
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    inner: Arc<Inner>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancelled() -> Self {
        let token = Self::default();
        token.cancel();
        token
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.flag.load(Ordering::SeqCst)
    }
    pub fn cancel(&self) {
        self.inner.flag.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }
    /// Completes once the token is cancelled (immediately if it already is).
    pub async fn wait_cancelled(&self) {
        loop {
            // Register interest before checking the flag so a concurrent
            // `cancel` cannot slip between the check and the wait.
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn wait_cancelled_wakes_on_cancel_and_when_already_cancelled() {
        let token = CancellationToken::new();
        let waiter = {
            let t = token.clone();
            tokio::spawn(async move { t.wait_cancelled().await })
        };
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        token.cancel();
        waiter.await.unwrap();
        CancellationToken::cancelled().wait_cancelled().await;
    }

    #[test]
    fn cancel_propagates_to_clones() {
        let token = CancellationToken::new();
        let clone = token.clone();
        token.cancel();
        assert!(clone.is_cancelled());
        assert!(token.is_cancelled());
    }
}

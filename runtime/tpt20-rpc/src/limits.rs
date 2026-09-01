use crate::context::RpcContext;
use crate::metadata::Metadata;
use crate::peer::PeerInfo;
use crate::status::{RpcError, Status};
use std::time::{Duration, Instant};
use thiserror::Error;

pub trait RateLimiter {
    fn check(&self, peer: &PeerInfo, method: &str) -> Result<(), RateLimitError>;
}

#[derive(Debug, Clone)]
pub struct TokenBucketRateLimiter {
    pub max_requests: u64,
    pub window: Duration,
    pub counts: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, (u64, Instant)>>>,
}

impl TokenBucketRateLimiter {
    pub fn new(max_requests: u64, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            counts: std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new())),
        }
    }
}

impl RateLimiter for TokenBucketRateLimiter {
    fn check(&self, peer: &PeerInfo, _method: &str) -> Result<(), RateLimitError> {
        let mut counts = self.counts.lock().unwrap();
        let now = Instant::now();
        let key = &peer.addr;

        if let Some((count, last)) = counts.get_mut(key) {
            if now.duration_since(*last) > self.window {
                *count = 1;
                *last = now;
                return Ok(());
            }
            *count += 1;
            if *count > self.max_requests {
                return Err(RateLimitError::Exceeded);
            }
        } else {
            counts.insert(key.clone(), (1, now));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CompositeRateLimiter {
    pub limiters: Vec<Box<dyn RateLimiter + Send + Sync>>,
}

impl CompositeRateLimiter {
    pub fn new(limiters: Vec<Box<dyn RateLimiter + Send + Sync>>) -> Self {
        Self { limiters }
    }
}

impl RateLimiter for CompositeRateLimiter {
    fn check(&self, peer: &PeerInfo, method: &str) -> Result<(), RateLimitError> {
        for limiter in &self.limiters {
            limiter.check(peer, method)?;
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum RateLimitError {
    #[error("rate limit exceeded")]
    Exceeded,
}

impl From<RateLimitError> for RpcError {
    fn from(_: RateLimitError) -> Self {
        RpcError::Status(Status::ResourceExhausted)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RequestLimits {
    pub max_message_bytes: usize,
    pub max_header_bytes: usize,
    pub max_metadata_bytes: usize,
    pub max_method_len: usize,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: 4 * 1024 * 1024,
            max_header_bytes: 8 * 1024,
            max_metadata_bytes: 8 * 1024,
            max_method_len: 256,
        }
    }
}

impl RequestLimits {
    pub fn check_message(&self, len: usize) -> Result<(), RpcError> {
        if len > self.max_message_bytes {
            Err(RpcError::ResourceExhausted)
        } else {
            Ok(())
        }
    }

    pub fn check_header(&self, len: usize) -> Result<(), RpcError> {
        if len > self.max_header_bytes {
            Err(RpcError::ResourceExhausted)
        } else {
            Ok(())
        }
    }

    pub fn check_metadata(&self, metadata: &Metadata) -> Result<(), RpcError> {
        if metadata.total_bytes() > self.max_metadata_bytes {
            Err(RpcError::ResourceExhausted)
        } else {
            Ok(())
        }
    }

    pub fn check_method(&self, method: &str) -> Result<(), RpcError> {
        if method.len() > self.max_method_len {
            Err(RpcError::InvalidArgument("method name too long"))
        } else {
            Ok(())
        }
    }
}

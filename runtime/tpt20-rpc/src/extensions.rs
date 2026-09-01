//! Extension storage for RPC contexts (spec §16.1).

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct Extensions {
    inner: HashMap<String, Arc<dyn Any + Send + Sync>>,
}

impl Extensions {
    pub fn new() -> Self { Self::default() }
    pub fn insert<T: Any + Send + Sync>(&mut self, key: impl Into<String>, value: T) {
        self.inner.insert(key.into(), Arc::new(value));
    }
    pub fn get<T: Any + Send + Sync>(&self, key: &str) -> Option<&T> {
        self.inner.get(key).and_then(|v| v.downcast_ref::<T>())
    }
    pub fn remove<T: Any + Send + Sync>(&mut self, key: &str) -> Option<Arc<T>> {
        self.inner.remove(key).and_then(|v| Arc::downcast(v).ok())
    }
    pub fn contains_key(&self, key: &str) -> bool { self.inner.contains_key(key) }
    pub fn len(&self) -> usize { self.inner.len() }
    pub fn is_empty(&self) -> bool { self.inner.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extensions_roundtrip() {
        let mut exts = Extensions::new();
        exts.insert("my_val", 42u32);
        assert_eq!(exts.get::<u32>("my_val"), Some(&42));
    }
}

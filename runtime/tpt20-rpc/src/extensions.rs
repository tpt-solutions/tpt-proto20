//! Extension storage for RPC contexts (spec §16.1).

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// Type-erased extension storage for an RPC context.
pub type Extensions = HashMap<String, Arc<dyn Any + Send + Sync>>;

impl Extensions {
    /// Inserts an extension value.
    pub fn insert<T: Any + Send + Sync>(&mut self, key: impl Into<String>, value: T) {
        self.insert(key.into(), Arc::new(value));
    }

    /// Gets an extension value by key, downcast to the requested type.
    pub fn get<T: Any + Send + Sync>(&self, key: &str) -> Option<&T> {
        self.get(key)
            .and_then(|v| v.downcast_ref::<T>())
    }

    /// Removes an extension value by key.
    pub fn remove<T: Any + Send + Sync>(&mut self, key: &str) -> Option<Arc<T>> {
        self.remove(key)
            .and_then(|v| Arc::downcast(v).ok())
    }

    /// Returns true if the extension exists.
    pub fn contains_key(&self, key: &str) -> bool {
        self.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_roundtrip() {
        let mut exts = Extensions::new();
        exts.insert("my_val", 42u32);
        assert_eq!(exts.get::<u32>("my_val"), Some(&42));
        assert!(exts.get::<String>("my_val").is_none());
    }
}

//! Metadata type for RPC calls.
//!
//! Metadata is carried alongside RPC calls and follows these rules (spec §16.5):
//! - keys should be lowercase
//! - binary metadata uses a standard suffix convention
//! - metadata size limits are enforced by callers
//! - reserved metadata keys are protected by callers

use std::collections::HashMap;

/// RPC metadata: a map from lowercase keys to lists of values.
///
/// Binary metadata values may use a `-bin` suffix convention per spec §16.5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    inner: HashMap<String, Vec<String>>,
}

impl Metadata {
    /// Creates empty metadata.
    pub fn new() -> Self {
        Metadata {
            inner: HashMap::new(),
        }
    }

    /// Inserts a metadata value.
    ///
    /// Values are stored in insertion order per key.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.inner.entry(key.into().to_ascii_lowercase()).or_default().push(value.into());
    }

    /// Returns all values for a key.
    pub fn get(&self, key: &str) -> Option<&[String]> {
        self.inner.get(key).map(|v| v.as_slice())
    }

    /// Removes all values for a key.
    pub fn remove(&mut self, key: &str) {
        self.inner.remove(key);
    }

    /// Returns an iterator over key-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.inner.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Returns the number of keys.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if there are no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Merges another metadata map into this one.
    pub fn merge(&mut self, other: Metadata) {
        for (key, values) in other.inner {
            self.inner.entry(key).or_default().extend(values);
        }
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self::new()
    }
}

impl Extend<(String, String)> for Metadata {
    fn extend<T: IntoIterator<Item = (String, String)>>(&mut self, iter: T) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_basic() {
        let mut m = Metadata::new();
        m.insert("key", "value");
        assert_eq!(m.get("key"), Some(&["value"][..]));
    }

    #[test]
    fn metadata_lowercase() {
        let mut m = Metadata::new();
        m.insert("KEY", "value");
        assert_eq!(m.get("key"), Some(&["value"][..]));
    }

    #[test]
    fn metadata_multiple_values() {
        let mut m = Metadata::new();
        m.insert("key", "v1");
        m.insert("key", "v2");
        assert_eq!(m.get("key"), Some(&["v1", "v2"][..]));
    }
}

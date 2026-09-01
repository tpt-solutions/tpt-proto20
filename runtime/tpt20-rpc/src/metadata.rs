//! Metadata handling for RPC calls (spec §16.5).

use std::collections::HashMap;
use std::fmt;

use thiserror::Error;

use crate::stream::{ReceiveError, SendError};

/// Error that occurs during metadata validation or manipulation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MetadataError {
    /// Metadata keys must be lowercase.
    #[error("metadata key must be lowercase")]
    KeyNotLowercase,
    /// Key uses a reserved prefix.
    #[error("metadata key uses reserved prefix: {0}")]
    ReservedKeyPrefix(String),
    /// Binary metadata key must end with `-bin`.
    #[error("binary metadata key must end with '-bin'")]
    BinaryKeySuffixMissing,
    /// Metadata size limit exceeded.
    #[error("metadata size limit exceeded ({limit} bytes)")]
    SizeLimitExceeded { limit: usize },
    /// Key is empty.
    #[error("metadata key is empty")]
    EmptyKey,
}

/// A metadata key, enforced to be lowercase.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetadataKey(String);

impl MetadataKey {
    /// Creates a new metadata key, enforcing lowercase and non-empty.
    pub fn new(key: impl Into<String>) -> Result<Self, MetadataError> {
        let key = key.into();
        if key.is_empty() {
            return Err(MetadataError::EmptyKey);
        }
        if key != key.to_lowercase() {
            return Err(MetadataError::KeyNotLowercase);
        }
        Ok(Self(key))
    }

    /// Returns the key as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MetadataKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<MetadataKey> for String {
    fn from(key: MetadataKey) -> Self {
        key.0
    }
}

impl<'a> From<&'a MetadataKey> for &'a str {
    fn from(key: &MetadataKey) -> Self {
        &key.0
    }
}

/// A metadata value, either text or binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataValue {
    /// A text metadata value (must be valid UTF-8).
    Text(String),
    /// A binary metadata value (arbitrary bytes).
    Binary(Vec<u8>),
}

impl MetadataValue {
    /// Creates a text metadata value.
    pub fn text(value: impl Into<String>) -> Self {
        MetadataValue::Text(value.into())
    }

    /// Creates a binary metadata value.
    pub fn binary(value: impl Into<Vec<u8>>) -> Self {
        MetadataValue::Binary(value.into())
    }
}

impl From<String> for MetadataValue {
    fn from(value: String) -> Self {
        MetadataValue::Text(value)
    }
}

impl From<Vec<u8>> for MetadataValue {
    fn from(value: Vec<u8>) -> Self {
        MetadataValue::Binary(value)
    }
}

impl<'a> From<&'a str> for MetadataValue {
    fn from(value: &str) -> Self {
        MetadataValue::Text(value.to_string())
    }
}

impl<'a> From<&'a [u8]> for MetadataValue {
    fn from(value: &[u8]) -> Self {
        MetadataValue::Binary(value.to_vec())
    }
}

/// A collection of metadata key-value pairs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    inner: HashMap<MetadataKey, MetadataValue>,
    size_limit: usize,
    current_size: usize,
}

impl Metadata {
    /// Creates an empty metadata collection with the given size limit.
    pub fn new(size_limit: usize) -> Self {
        Self {
            inner: HashMap::new(),
            size_limit,
            current_size: 0,
        }
    }

    /// Creates an empty metadata collection with the default size limit (8 KiB).
    pub fn with_default_limit() -> Self {
        Self::new(8192)
    }

    /// Inserts a metadata entry, enforcing the size limit.
    pub fn insert(&mut self, key: MetadataKey, value: MetadataValue) -> Result<(), MetadataError> {
        let entry_size = key.as_str().len() + match &value {
            MetadataValue::Text(v) => v.len(),
            MetadataValue::Binary(v) => v.len(),
        };

        if self.current_size + entry_size > self.size_limit {
            return Err(MetadataError::SizeLimitExceeded {
                limit: self.size_limit,
            });
        }

        // If replacing an existing key, subtract the old size first.
        if let Some(old) = self.inner.insert(key.clone(), value) {
            let old_size = key.as_str().len() + match &old {
                MetadataValue::Text(v) => v.len(),
                MetadataValue::Binary(v) => v.len(),
            };
            self.current_size = self.current_size.saturating_sub(old_size);
        }

        self.current_size += entry_size;
        Ok(())
    }

    /// Inserts a text value, enforcing lowercase key.
    pub fn insert_text(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), MetadataError> {
        let key = MetadataKey::new(key)?;
        self.insert(key, MetadataValue::text(value))
    }

    /// Inserts a binary value, enforcing `-bin` suffix on the key.
    pub fn insert_binary(
        &mut self,
        key: impl Into<String>,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), MetadataError> {
        let key_str = key.into();
        if !key_str.ends_with("-bin") {
            return Err(MetadataError::BinaryKeySuffixMissing);
        }
        let key = MetadataKey::new(key_str)?;
        self.insert(key, MetadataValue::binary(value))
    }

    /// Returns the value for a key, if present.
    pub fn get(&self, key: &str) -> Option<&MetadataValue> {
        self.inner.get(&MetadataKey::new(key).ok()?).or_else(|| {
            self.inner
                .keys()
                .find(|k| k.as_str().eq_ignore_ascii_case(key))
                .and_then(|k| self.inner.get(k))
        })
    }

    /// Returns true if the metadata contains the given key.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Removes a key, returning its value if present.
    pub fn remove(&mut self, key: &str) -> Option<MetadataValue> {
        let key_ok = MetadataKey::new(key).ok();
        let result = key_ok.and_then(|k| {
            let val = self.inner.remove(&k);
            if let Some(ref v) = val {
                let size = key.len() + match v {
                    MetadataValue::Text(t) => t.len(),
                    MetadataValue::Binary(b) => b.len(),
                };
                self.current_size = self.current_size.saturating_sub(size);
            }
            val
        });
        result
    }

    /// Returns an iterator over all entries.
    pub fn iter(&self) -> impl Iterator<Item = (&MetadataKey, &MetadataValue)> {
        self.inner.iter()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if the metadata is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_text_enforces_lowercase() {
        let mut md = Metadata::new(1024);
        assert!(md.insert_text("x-request-id", "abc").is_ok());
        assert!(md.insert_text("X-Request-Id", "def").is_err());
    }

    #[test]
    fn insert_binary_enforces_suffix() {
        let mut md = Metadata::new(1024);
        assert!(md.insert_binary("x-data-bin", b"hello").is_ok());
        assert!(md.insert_binary("x-data", b"hello").is_err());
    }

    #[test]
    fn size_limit_enforced() {
        let mut md = Metadata::new(10);
        assert!(md.insert_text("k", "abc").is_ok());
        assert!(matches!(
            md.insert_text("k2", "defghij"),
            Err(MetadataError::SizeLimitExceeded { .. })
        ));
    }

    #[test]
    fn remove_adjusts_size() {
        let mut md = Metadata::new(1024);
        md.insert_text("k", "abc").unwrap();
        md.remove("k");
        assert!(md.is_empty());
        assert_eq!(md.current_size, 0);
    }

    #[test]
    fn case_insensitive_get() {
        let mut md = Metadata::new(1024);
        md.insert_text("x-request-id", "abc").unwrap();
        assert_eq!(md.get("x-request-id").map(|v| v.as_ref()), Some(MetadataValue::text("abc")).as_ref().map(|v| v.as_ref()));
        assert_eq!(md.get("X-Request-Id").map(|v| v.as_ref()), Some(MetadataValue::text("abc")).as_ref().map(|v| v.as_ref()));
    }
}

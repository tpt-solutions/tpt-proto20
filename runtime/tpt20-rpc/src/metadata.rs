//! Metadata handling for RPC calls (spec §16.5).

use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MetadataError {
    #[error("metadata key must be lowercase")]
    KeyNotLowercase,
    #[error("metadata key uses reserved prefix: {0}")]
    ReservedKeyPrefix(String),
    #[error("binary metadata key must end with '-bin'")]
    BinaryKeySuffixMissing,
    #[error("metadata size limit exceeded ({limit} bytes)")]
    SizeLimitExceeded { limit: usize },
    #[error("metadata key is empty")]
    EmptyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetadataKey(String);

impl MetadataKey {
    pub fn new(key: impl Into<String>) -> Result<Self, MetadataError> {
        let key = key.into();
        if key.is_empty() { return Err(MetadataError::EmptyKey); }
        if key != key.to_lowercase() { return Err(MetadataError::KeyNotLowercase); }
        Ok(Self(key))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl fmt::Display for MetadataKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<MetadataKey> for String {
    fn from(key: MetadataKey) -> Self { key.0 }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataValue {
    Text(String),
    Binary(Vec<u8>),
}

impl MetadataValue {
    pub fn text(value: impl Into<String>) -> Self { MetadataValue::Text(value.into()) }
    pub fn binary(value: impl Into<Vec<u8>>) -> Self { MetadataValue::Binary(value.into()) }
    pub fn is_text(&self) -> bool { matches!(self, MetadataValue::Text(_)) }
    pub fn is_binary(&self) -> bool { matches!(self, MetadataValue::Binary(_)) }
}

impl From<String> for MetadataValue { fn from(v: String) -> Self { MetadataValue::Text(v) } }
impl From<Vec<u8>> for MetadataValue { fn from(v: Vec<u8>) -> Self { MetadataValue::Binary(v) } }
impl<'a> From<&'a str> for MetadataValue { fn from(v: &str) -> Self { MetadataValue::Text(v.to_string()) } }
impl<'a> From<&'a [u8]> for MetadataValue { fn from(v: &[u8]) -> Self { MetadataValue::Binary(v.to_vec()) } }

impl AsRef<str> for MetadataValue {
    fn as_ref(&self) -> &str {
        match self { MetadataValue::Text(s) => s.as_str(), MetadataValue::Binary(_) => "" }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    inner: HashMap<MetadataKey, MetadataValue>,
    size_limit: usize,
    current_size: usize,
}

impl Metadata {
    pub fn new(size_limit: usize) -> Self {
        Self { inner: HashMap::new(), size_limit, current_size: 0 }
    }
    pub fn with_default_limit() -> Self { Self::new(8192) }

    pub fn insert(&mut self, key: MetadataKey, value: MetadataValue) -> Result<(), MetadataError> {
        let entry_size = key.as_str().len() + match &value {
            MetadataValue::Text(v) => v.len(),
            MetadataValue::Binary(v) => v.len(),
        };
        if self.current_size + entry_size > self.size_limit {
            return Err(MetadataError::SizeLimitExceeded { limit: self.size_limit });
        }
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

    pub fn insert_text(&mut self, key: impl Into<String>, value: impl Into<String>) -> Result<(), MetadataError> {
        let key = MetadataKey::new(key)?;
        self.insert(key, MetadataValue::text(value))
    }

    pub fn insert_binary(&mut self, key: impl Into<String>, value: impl Into<Vec<u8>>) -> Result<(), MetadataError> {
        let key_str = key.into();
        if !key_str.ends_with("-bin") { return Err(MetadataError::BinaryKeySuffixMissing); }
        let key = MetadataKey::new(key_str)?;
        self.insert(key, MetadataValue::binary(value))
    }

    pub fn get(&self, key: &str) -> Option<&MetadataValue> {
        self.inner.get(&MetadataKey::new(key).ok()?).or_else(|| {
            self.inner.keys().find(|k| k.as_str().eq_ignore_ascii_case(key)).and_then(|k| self.inner.get(k))
        })
    }

    pub fn contains_key(&self, key: &str) -> bool { self.get(key).is_some() }

    pub fn get_first_text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| match v {
            MetadataValue::Text(s) => s.split(',').next().map(|s| s.trim()),
            MetadataValue::Binary(_) => None,
        })
    }

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

    pub fn iter(&self) -> impl Iterator<Item = (&MetadataKey, &MetadataValue)> { self.inner.iter() }
    pub fn len(&self) -> usize { self.inner.len() }
    pub fn is_empty(&self) -> bool { self.inner.is_empty() }
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
        assert!(matches!(md.insert_text("k2", "defghij"), Err(MetadataError::SizeLimitExceeded { .. })));
    }
}

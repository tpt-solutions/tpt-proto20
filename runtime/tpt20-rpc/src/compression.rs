//! Compression support for RPC (spec §16).

use std::time::Duration;

/// Supported compression algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompressionAlgorithm {
    /// No compression.
    None,
    /// GZIP compression.
    Gzip,
    /// DEFLATE compression.
    Deflate,
    /// Identity (no-op, used for signaling).
    Identity,
}

impl CompressionAlgorithm {
    /// Returns the canonical name for this algorithm.
    pub const fn as_str(&self) -> &'static str {
        match self {
            CompressionAlgorithm::None => "identity",
            CompressionAlgorithm::Gzip => "gzip",
            CompressionAlgorithm::Deflate => "deflate",
            CompressionAlgorithm::Identity => "identity",
        }
    }

    /// Returns true if this algorithm performs actual compression.
    pub const fn is_compression(&self) -> bool {
        matches!(self, CompressionAlgorithm::Gzip | CompressionAlgorithm::Deflate)
    }
}

impl std::fmt::Display for CompressionAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<&str> for CompressionAlgorithm {
    type Error = UnknownCompressionAlgorithm;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        match name {
            "identity" | "" => Ok(CompressionAlgorithm::None),
            "gzip" => Ok(CompressionAlgorithm::Gzip),
            "deflate" => Ok(CompressionAlgorithm::Deflate),
            _ => Err(UnknownCompressionAlgorithm(name.to_string())),
        }
    }
}

/// Error returned when an unknown compression algorithm is encountered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCompressionAlgorithm(pub String);

impl std::fmt::Display for UnknownCompressionAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown compression algorithm: {}", self.0)
    }
}

impl std::error::Error for UnknownCompressionAlgorithm {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_algorithm_names() {
        assert_eq!(CompressionAlgorithm::Gzip.as_str(), "gzip");
        assert_eq!(CompressionAlgorithm::None.as_str(), "identity");
        assert!(CompressionAlgorithm::Gzip.is_compression());
        assert!(!CompressionAlgorithm::None.is_compression());
    }
}

//! Endpoint configuration for transports.
//!
//! An [`Endpoint`] describes how to connect to or listen on a transport.

use crate::metadata::Metadata;

/// TLS configuration for a transport endpoint.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to the PEM-encoded certificate chain (server) or CA certs (client).
    pub cert_path: Option<std::path::PathBuf>,
    /// Path to the PEM-encoded private key.
    pub key_path: Option<std::path::PathBuf>,
    /// Raw PEM-encoded certificate chain bytes (alternative to `cert_path`).
    pub cert_pem: Option<Vec<u8>>,
    /// Raw PEM-encoded private key bytes (alternative to `key_path`).
    pub key_pem: Option<Vec<u8>>,
    /// ALPN protocols to advertise (e.g. `["h2"]` for HTTP/2).
    pub alpn_protocols: Vec<Vec<u8>>,
    /// If true, skip certificate verification (development only).
    pub accept_invalid_certs: bool,
    /// Client CA certificate chain for mTLS server verification.
    pub client_ca_pem: Option<Vec<u8>>,
    /// Client CA certificate path for mTLS server verification.
    pub client_ca_path: Option<std::path::PathBuf>,
    /// If true, require client certificates (mTLS).
    pub require_client_cert: bool,
}

impl TlsConfig {
    /// Creates a TLS config with ALPN set to HTTP/2.
    pub fn http2() -> Self {
        TlsConfig {
            cert_path: None,
            key_path: None,
            cert_pem: None,
            key_pem: None,
            alpn_protocols: vec![b"h2".to_vec()],
            accept_invalid_certs: false,
            client_ca_pem: None,
            client_ca_path: None,
            require_client_cert: false,
        }
    }

    /// Sets the ALPN protocols.
    pub fn with_alpn(mut self, protocols: impl IntoIterator<Item = impl Into<Vec<u8>>>) -> Self {
        self.alpn_protocols = protocols.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the certificate and key paths.
    pub fn with_pem_paths(mut self, cert: impl Into<std::path::PathBuf>, key: impl Into<std::path::PathBuf>) -> Self {
        self.cert_path = Some(cert.into());
        self.key_path = Some(key.into());
        self
    }

    /// Enables mTLS with the given client CA certificate path.
    pub fn with_client_ca_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.client_ca_path = Some(path.into());
        self.require_client_cert = true;
        self
    }

    /// Enables mTLS with the given client CA certificate PEM bytes.
    pub fn with_client_ca_pem(mut self, pem: impl Into<Vec<u8>>) -> Self {
        self.client_ca_pem = Some(pem.into());
        self.require_client_cert = true;
        self
    }

    /// Enables mTLS mode (require client certificates).
    pub fn require_client_cert(mut self, require: bool) -> Self {
        self.require_client_cert = require;
        self
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self::http2()
    }
}

/// A transport endpoint: where to connect or listen.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// The target address, e.g. `"127.0.0.1:50051"` or `"localhost:50051"`.
    pub address: String,
    /// Optional TLS configuration.
    pub tls: Option<TlsConfig>,
    /// Optional default metadata applied to every call.
    pub default_metadata: Metadata,
    /// Maximum message size in bytes (None = crate default).
    pub max_message_bytes: Option<usize>,
}

impl Endpoint {
    /// Creates a new endpoint from an address string.
    pub fn new(address: impl Into<String>) -> Self {
        Endpoint {
            address: address.into(),
            tls: None,
            default_metadata: Metadata::new(),
            max_message_bytes: None,
        }
    }

    /// Sets TLS configuration.
    pub fn with_tls(mut self, tls: TlsConfig) -> Self {
        self.tls = Some(tls);
        self
    }

    /// Enables TLS with the given certificate and key paths.
    pub fn with_tls_pem(self, cert: impl Into<std::path::PathBuf>, key: impl Into<std::path::PathBuf>) -> Self {
        self.with_tls(TlsConfig::default().with_pem_paths(cert, key))
    }

    /// Adds default metadata for every call made through this endpoint.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.default_metadata.insert(key, value);
        self
    }

    /// Sets the maximum message size in bytes.
    pub fn with_max_message_bytes(mut self, max: usize) -> Self {
        self.max_message_bytes = Some(max);
        self
    }

    /// Returns true if this endpoint uses TLS.
    pub fn uses_tls(&self) -> bool {
        self.tls.is_some()
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.uses_tls() {
            write!(f, "https://{}", self.address)
        } else {
            write!(f, "http://{}", self.address)
        }
    }
}

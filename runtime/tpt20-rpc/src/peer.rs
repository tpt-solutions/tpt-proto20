use crate::status::Status;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub addr: String,
    pub tls_peer: Option<TlsPeerIdentity>,
}

impl PeerInfo {
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            tls_peer: None,
        }
    }

    pub fn with_tls(mut self, tls_peer: TlsPeerIdentity) -> Self {
        self.tls_peer = Some(tls_peer);
        self
    }
}

#[derive(Debug, Clone)]
pub struct TlsPeerIdentity {
    pub subject: String,
    pub issuer: String,
    pub san: Vec<String>,
}

impl TlsPeerIdentity {
    pub fn new(subject: impl Into<String>, issuer: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            issuer: issuer.into(),
            san: Vec::new(),
        }
    }

    pub fn with_san(mut self, san: Vec<String>) -> Self {
        self.san = san;
        self
    }
}

pub trait PeerInspector {
    fn inspect(&self, peer: &PeerInfo) -> Result<(), PeerInspectionError>;
}

#[derive(Debug, Error)]
pub enum PeerInspectionError {
    #[error("peer address not allowed: {0}")]
    AddressNotAllowed(String),
    #[error("TLS peer not authenticated")]
    NotAuthenticated,
    #[error("peer certificate invalid: {0}")]
    InvalidCertificate(String),
    #[error("peer not authorized")]
    NotAuthorized,
}

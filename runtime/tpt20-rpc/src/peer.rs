//! Peer information for an RPC connection (spec §16.1).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub addr: String,
    pub port: u16,
    pub identity: Option<String>,
}

impl PeerInfo {
    pub fn new(addr: impl Into<String>, port: u16) -> Self {
        Self { addr: addr.into(), port, identity: None }
    }
    pub fn with_identity(mut self, identity: impl Into<String>) -> Self {
        self.identity = Some(identity.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peer_info() {
        let peer = PeerInfo::new("10.0.0.1", 8080).with_identity("client-1");
        assert_eq!(peer.addr, "10.0.0.1");
        assert_eq!(peer.port, 8080);
    }
}

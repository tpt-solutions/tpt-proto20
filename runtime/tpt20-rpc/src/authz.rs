use crate::context::RpcContext;
use crate::metadata::Metadata;
use crate::peer::PeerInfo;
use crate::status::{RpcError, Status};
use thiserror::Error;

pub trait Authorizer {
    fn authorize(&self, ctx: &RpcContext, method: &str) -> Result<(), AuthzError>;
}

#[derive(Debug, Clone)]
pub struct AllowAllAuthorizer;

impl Authorizer for AllowAllAuthorizer {
    fn authorize(&self, _ctx: &RpcContext, _method: &str) -> Result<(), AuthzError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RoleBasedAuthorizer {
    pub allowed_roles: Vec<String>,
}

impl RoleBasedAuthorizer {
    pub fn new(allowed_roles: Vec<String>) -> Self {
        Self { allowed_roles }
    }
}

impl Authorizer for RoleBasedAuthorizer {
    fn authorize(&self, ctx: &RpcContext, _method: &str) -> Result<(), AuthzError> {
        let roles = ctx
            .metadata
            .get_first("x-role")
            .and_then(|v| std::str::from_utf8(v).ok())
            .map(|s| s.split(',').collect::<Vec<_>>())
            .unwrap_or_default();

        for role in &self.allowed_roles {
            if roles.contains(&role.as_str()) {
                return Ok(());
            }
        }
        Err(AuthzError::RoleNotAllowed)
    }
}

#[derive(Debug, Clone)]
pub struct AclAuthorizer {
    pub rules: Vec<AclRule>,
}

#[derive(Debug, Clone)]
pub struct AclRule {
    pub method_prefix: String,
    pub allowed_peers: Vec<String>,
    pub allowed_roles: Vec<String>,
}

impl AclAuthorizer {
    pub fn new(rules: Vec<AclRule>) -> Self {
        Self { rules }
    }

    pub fn allow(method_prefix: impl Into<String>, peer: impl Into<String>) -> Self {
        Self {
            rules: vec![AclRule {
                method_prefix: method_prefix.into(),
                allowed_peers: vec![peer.into()],
                allowed_roles: Vec::new(),
            }],
        }
    }
}

impl Authorizer for AclAuthorizer {
    fn authorize(&self, ctx: &RpcContext, method: &str) -> Result<(), AuthzError> {
        let peer = ctx
            .peer
            .as_ref()
            .map(|p| p.addr.as_str())
            .unwrap_or("unknown");

        for rule in &self.rules {
            if method.starts_with(&rule.method_prefix) {
                if rule.allowed_peers.is_empty() || rule.allowed_peers.iter().any(|p| p == peer) {
                    if rule.allowed_roles.is_empty() {
                        return Ok(());
                    }
                    let roles = ctx
                        .metadata
                        .get_first("x-role")
                        .and_then(|v| std::str::from_utf8(v).ok())
                        .map(|s| s.split(',').collect::<Vec<_>>())
                        .unwrap_or_default();
                    for role in &rule.allowed_roles {
                        if roles.contains(&role.as_str()) {
                            return Ok(());
                        }
                    }
                    return Err(AuthzError::RoleNotAllowed);
                }
            }
        }
        Err(AuthzError::MethodNotAllowed)
    }
}

#[derive(Debug, Error)]
pub enum AuthzError {
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("role not allowed")]
    RoleNotAllowed,
    #[error("peer not allowed: {0}")]
    PeerNotAllowed(String),
    #[error("authorization failed: {0}")]
    Failed(String),
}

impl From<AuthzError> for RpcError {
    fn from(e: AuthzError) -> Self {
        match e {
            AuthzError::MethodNotAllowed | AuthzError::PeerNotAllowed(_) => RpcError::PermissionDenied,
            AuthzError::RoleNotAllowed => RpcError::PermissionDenied,
            AuthzError::Failed(_) => RpcError::Internal(e.to_string()),
        }
    }
}

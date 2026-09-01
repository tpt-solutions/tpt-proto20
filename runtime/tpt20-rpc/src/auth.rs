//! Authentication and authorization hooks (spec §16, §18.6).

use crate::{metadata::Metadata, RpcContext};

/// Error returned during authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthError {
    message: String,
}

impl AuthError {
    /// Creates a new authentication error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "authentication failed: {}", self.message)
    }
}

impl std::error::Error for AuthError {}

/// Error returned during authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthzError {
    message: String,
}

impl AuthzError {
    /// Creates a new authorization error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AuthzError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "authorization failed: {}", self.message)
    }
}

impl std::error::Error for AuthzError {}

/// Result of an authentication check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    /// The authenticated identity, if any.
    pub identity: Option<String>,
    /// Authentication metadata.
    pub metadata: Metadata,
}

impl AuthContext {
    /// Creates a new authentication context.
    pub fn new(identity: Option<String>, metadata: Metadata) -> Self {
        Self { identity, metadata }
    }
}

impl Default for AuthContext {
    fn default() -> Self {
        Self {
            identity: None,
            metadata: Metadata::with_default_limit(),
        }
    }
}

/// A hook that authenticates an incoming RPC.
pub trait Authenticator: Send + Sync {
    /// Authenticates the RPC, returning the caller's identity on success.
    fn authenticate(&self, ctx: &RpcContext) -> Result<AuthContext, AuthError>;
}

impl<F> Authenticator for F
where
    F: Fn(&RpcContext) -> Result<AuthContext, AuthError> + Send + Sync,
{
    fn authenticate(&self, ctx: &RpcContext) -> Result<AuthContext, AuthError> {
        self(ctx)
    }
}

/// A hook that authorizes an authenticated RPC.
pub trait Authorizer: Send + Sync {
    /// Authorizes the RPC, returning true if allowed.
    fn authorize(&self, ctx: &RpcContext, auth: &AuthContext) -> Result<bool, AuthzError>;
}

impl<F> Authorizer for F
where
    F: Fn(&RpcContext, &AuthContext) -> Result<bool, AuthzError> + Send + Sync,
{
    fn authorize(&self, ctx: &RpcContext, auth: &AuthContext) -> Result<bool, AuthzError> {
        self(ctx, auth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticator_closure() {
        let auth = |ctx: &RpcContext| {
            if ctx.metadata().contains_key("authorization") {
                Ok(AuthContext::new(Some("user".into()), Metadata::with_default_limit()))
            } else {
                Err(AuthError::new("missing token"))
            }
        };

        let mut ctx = RpcContext::new();
        assert!(auth(&ctx).is_err());

        ctx.metadata_mut()
            .insert_text("authorization", "token")
            .unwrap();
        let result = auth(&ctx);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().identity, Some("user".into()));
    }
}

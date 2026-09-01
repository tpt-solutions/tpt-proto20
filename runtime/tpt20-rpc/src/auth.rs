//! Authentication and authorization hooks (spec §16, §18.6).

use crate::metadata::Metadata;
use crate::RpcContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthError {
    message: String,
}

impl AuthError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "authentication failed: {}", self.message)
    }
}

impl std::error::Error for AuthError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub identity: Option<String>,
    pub metadata: Metadata,
}

impl AuthContext {
    pub fn new(identity: Option<String>, metadata: Metadata) -> Self {
        Self { identity, metadata }
    }
}

impl Default for AuthContext {
    fn default() -> Self {
        Self { identity: None, metadata: Metadata::with_default_limit() }
    }
}

pub trait Authenticator: Send + Sync {
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

#[derive(Debug, Clone)]
pub struct TokenAuthenticator {
    validator: fn(&str) -> bool,
}

impl TokenAuthenticator {
    pub fn new(validator: fn(&str) -> bool) -> Self {
        Self { validator }
    }
}

impl Authenticator for TokenAuthenticator {
    fn authenticate(&self, ctx: &RpcContext) -> Result<AuthContext, AuthError> {
        let token = ctx.metadata().get("authorization")
            .or_else(|| ctx.metadata().get("token"))
            .and_then(|v| match v {
                crate::MetadataValue::Text(t) => Some(t.as_str()),
                crate::MetadataValue::Binary(_) => None,
            })
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(|| AuthError::new("missing or invalid authorization token"))?;
        if (self.validator)(token) {
            Ok(AuthContext::new(Some(token.to_string()), Metadata::with_default_limit()))
        } else {
            Err(AuthError::new("invalid authorization token"))
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetadataAuthenticator {
    required_keys: Vec<String>,
    validators: std::collections::BTreeMap<String, fn(&[u8]) -> bool>,
}

impl MetadataAuthenticator {
    pub fn new(required_keys: Vec<String>) -> Self {
        Self { required_keys, validators: std::collections::BTreeMap::new() }
    }
    pub fn with_validator(mut self, key: impl Into<String>, validator: fn(&[u8]) -> bool) -> Self {
        self.validators.insert(key.into(), validator);
        self
    }
}

impl Authenticator for MetadataAuthenticator {
    fn authenticate(&self, ctx: &RpcContext) -> Result<AuthContext, AuthError> {
        for key in &self.required_keys {
            if ctx.metadata().get(key).is_none() {
                return Err(AuthError::new(format!("missing required metadata key: {}", key)));
            }
            if let Some(validator) = self.validators.get(key) {
                if let Some(crate::MetadataValue::Binary(b)) = ctx.metadata().get(key) {
                    if !validator(b) {
                        return Err(AuthError::new(format!("invalid metadata value for key: {}", key)));
                    }
                }
            }
        }
        Ok(AuthContext::new(
            ctx.metadata().get("x-identity").and_then(|v| match v {
                crate::MetadataValue::Text(t) => Some(t.clone()),
                crate::MetadataValue::Binary(_) => None,
            }),
            Metadata::with_default_limit(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthzError {
    message: String,
}

impl AuthzError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for AuthzError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "authorization failed: {}", self.message)
    }
}

impl std::error::Error for AuthzError {}

pub trait Authorizer: Send + Sync {
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

#[derive(Debug, Clone, Default)]
pub struct AllowAllAuthorizer;

impl Authorizer for AllowAllAuthorizer {
    fn authorize(&self, _ctx: &RpcContext, _auth: &AuthContext) -> Result<bool, AuthzError> {
        Ok(true)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DenyAllAuthorizer;

impl Authorizer for DenyAllAuthorizer {
    fn authorize(&self, _ctx: &RpcContext, _auth: &AuthContext) -> Result<bool, AuthzError> {
        Ok(false)
    }
}

#[derive(Debug, Clone)]
pub struct AclAuthorizer {
    required_roles: Vec<String>,
}

impl AclAuthorizer {
    pub fn new(required_roles: Vec<String>) -> Self {
        Self { required_roles }
    }
}

impl Authorizer for AclAuthorizer {
    fn authorize(&self, _ctx: &RpcContext, auth: &AuthContext) -> Result<bool, AuthzError> {
        let identity = auth.identity.as_deref().unwrap_or("");
        for role in &self.required_roles {
            if identity == role.as_str() { return Ok(true); }
        }
        Ok(false)
    }
}

#[derive(Debug, Clone)]
pub struct RoleBasedAuthorizer {
    allowed_roles: Vec<String>,
    metadata_key: String,
}

impl RoleBasedAuthorizer {
    pub fn new(allowed_roles: Vec<String>, metadata_key: impl Into<String>) -> Self {
        Self { allowed_roles, metadata_key: metadata_key.into() }
    }
}

impl Authorizer for RoleBasedAuthorizer {
    fn authorize(&self, ctx: &RpcContext, _auth: &AuthContext) -> Result<bool, AuthzError> {
        let roles = ctx.metadata().get(&self.metadata_key)
            .and_then(|v| match v {
                crate::MetadataValue::Text(t) => Some(t.as_str()),
                crate::MetadataValue::Binary(_) => None,
            })
            .map(|s| s.split(',').map(|s| s.trim()).collect::<Vec<_>>())
            .unwrap_or_default();
        for role in &self.allowed_roles {
            if roles.contains(&role.as_str()) { return Ok(true); }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_authenticator_valid() {
        let auth = TokenAuthenticator::new(|t| t == "secret");
        let mut ctx = RpcContext::new();
        ctx.metadata_mut().insert_text("authorization", "Bearer secret").unwrap();
        let result = auth.authenticate(&ctx);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().identity, Some("secret".to_string()));
    }
    #[test]
    fn allow_all_authorizer() {
        let authz = AllowAllAuthorizer;
        let ctx = RpcContext::new();
        let auth = AuthContext::default();
        assert!(authz.authorize(&ctx, &auth).unwrap());
    }
}

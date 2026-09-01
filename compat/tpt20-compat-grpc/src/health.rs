//! Health-checking protocol support (spec §10.3).

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::GrpcError;

/// The health status of a service in the gRPC health protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServingStatus {
    /// The status is unknown.
    Unknown = 0,
    /// The service is currently healthy and serving requests.
    Serving = 1,
    /// The service is not serving requests.
    NotServing = 2,
    /// The requested service is not known to the server.
    ServiceUnknown = 3,
}

impl ServingStatus {
    /// Returns the gRPC numeric code for this status.
    pub const fn code(&self) -> i32 {
        *self as i32
    }

    /// Returns the canonical string name.
    pub const fn as_str(&self) -> &'static str {
        match self {
            ServingStatus::Unknown => "UNKNOWN",
            ServingStatus::Serving => "SERVING",
            ServingStatus::NotServing => "NOT_SERVING",
            ServingStatus::ServiceUnknown => "SERVICE_UNKNOWN",
        }
    }
}

impl std::fmt::Display for ServingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A registry of service health statuses for the gRPC health protocol.
///
/// Thread-safe and can be shared across handlers.
#[derive(Debug, Clone, Default)]
pub struct HealthRegistry {
    inner: Arc<RwLock<HashMap<String, ServingStatus>>>,
}

impl HealthRegistry {
    /// Creates a new empty health registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the health status for a service.
    ///
    /// If `service` is empty, this sets the overall server status.
    pub async fn set_status(&self, service: impl Into<String>, status: ServingStatus) {
        self.inner.write().await.insert(service.into(), status);
    }

    /// Returns the health status for a service.
    ///
    /// If the service is not registered, returns [`ServingStatus::ServiceUnknown`].
    /// If `service` is empty, returns the overall server status (defaults to
    /// [`ServingStatus::Serving`] if not explicitly set).
    pub async fn get_status(&self, service: &str) -> ServingStatus {
        let guard = self.inner.read().await;
        if service.is_empty() {
            guard.get("").copied().unwrap_or(ServingStatus::Serving)
        } else {
            guard.get(service).copied().unwrap_or(ServingStatus::ServiceUnknown)
        }
    }

    /// Removes a service from the registry.
    pub async fn remove(&self, service: &str) {
        self.inner.write().await.remove(service);
    }

    /// Lists all registered services.
    pub async fn list_services(&self) -> Vec<String> {
        self.inner.read().await.keys().cloned().collect()
    }
}

/// A gRPC health-check handler.
///
/// Handles `grpc.health.v1.Health/Check` requests.
#[derive(Debug, Clone)]
pub struct HealthHandler {
    registry: HealthRegistry,
}

impl HealthHandler {
    /// Creates a new health handler backed by the given registry.
    pub fn new(registry: HealthRegistry) -> Self {
        HealthHandler { registry }
    }

    /// Handles a health check request for the given service name.
    ///
    /// Returns the serving status encoded as a gRPC-framed
    /// `HealthCheckResponse` message.
    pub async fn check(&self, service: &str) -> Result<Vec<u8>, GrpcError> {
        let status = self.registry.get_status(service).await;
        self.encode_response(status)
    }

    /// Encodes a [`ServingStatus`] as a gRPC-framed `HealthCheckResponse`.
    fn encode_response(&self, status: ServingStatus) -> Result<Vec<u8>, GrpcError> {
        let payload = encode_health_response(status);
        crate::frame::encode_grpc_frame(&payload, false)
    }
}

/// Encodes a [`ServingStatus`] as a protobuf wire-format `HealthCheckResponse`.
///
/// `HealthCheckResponse { serving_status: <code> }`
/// Field 1, varint encoding.
fn encode_health_response(status: ServingStatus) -> Vec<u8> {
    let mut buf = Vec::new();
    let tag: u64 = ((1u32 << 3) | 0) as u64; // field 1, varint wire type
    encode_varint(&mut buf, tag);
    encode_varint(&mut buf, status.code() as u64);
    buf
}

fn encode_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_registry_default_is_serving() {
        let registry = HealthRegistry::new();
        assert_eq!(registry.get_status("").await, ServingStatus::Serving);
        assert_eq!(
            registry.get_status("unknown.service").await,
            ServingStatus::ServiceUnknown
        );
    }

    #[tokio::test]
    async fn health_registry_set_and_get() {
        let registry = HealthRegistry::new();
        registry
            .set_status("user.v1.UserService", ServingStatus::Serving)
            .await;
        assert_eq!(
            registry.get_status("user.v1.UserService").await,
            ServingStatus::Serving
        );
    }

    #[tokio::test]
    async fn health_handler_check_serving() {
        let registry = HealthRegistry::new();
        registry
            .set_status("my.service", ServingStatus::Serving)
            .await;
        let handler = HealthHandler::new(registry);
        let response = handler.check("my.service").await.unwrap();
        let (flags, payload) = crate::frame::decode_grpc_frame(&response).unwrap();
        assert!(!flags.is_compressed());
        assert!(!payload.is_empty());
    }

    #[test]
    fn serving_status_codes() {
        assert_eq!(ServingStatus::Unknown.code(), 0);
        assert_eq!(ServingStatus::Serving.code(), 1);
        assert_eq!(ServingStatus::NotServing.code(), 2);
        assert_eq!(ServingStatus::ServiceUnknown.code(), 3);
    }
}

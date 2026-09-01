//! Tracing span attributes for tpt20 (spec §19.2).
//!
//! These constants match the OpenTelemetry semantic conventions for RPC
//! (`rpc.system`, `rpc.service`, `rpc.method`, `rpc.status`) and add a
//! `rpc.schema_fingerprint` attribute for schema-aware tracing.

/// RPC system attribute name.
pub const RPC_SYSTEM: &str = "rpc.system";

/// RPC service attribute name.
pub const RPC_SERVICE: &str = "rpc.service";

/// RPC method attribute name.
pub const RPC_METHOD: &str = "rpc.method";

/// RPC status attribute name.
pub const RPC_STATUS: &str = "rpc.status";

/// Schema fingerprint attribute name.
pub const RPC_SCHEMA_FINGERPRINT: &str = "rpc.schema_fingerprint";

/// Canonical tpt20 RPC system value.
pub const RPC_SYSTEM_VALUE: &str = "tpt20";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_constants_are_non_empty() {
        assert!(!RPC_SYSTEM.is_empty());
        assert!(!RPC_SERVICE.is_empty());
        assert!(!RPC_METHOD.is_empty());
        assert!(!RPC_STATUS.is_empty());
        assert!(!RPC_SCHEMA_FINGERPRINT.is_empty());
        assert_eq!(RPC_SYSTEM_VALUE, "tpt20");
    }
}

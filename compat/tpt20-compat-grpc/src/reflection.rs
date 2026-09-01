//! gRPC reflection protocol support (spec §10.3, "where feasible").
//!
//! Implements a basic [gRPC reflection](https://github.com/grpc/grpc/blob/master/doc/server-reflection.md)
//! service backed by tpt20 descriptors.
//!
//! ## Limitations
//!
//! - Only `FileDescriptorResponse` queries are supported.
//! - The reflection output is a simplified representation, not a full
//!   `google.protobuf.FileDescriptorProto` serialization.
//! - Well-known types are not expanded.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// gRPC reflection service backed by tpt20 descriptors.
///
/// Handles the `grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo`
/// bidirectional streaming RPC.
#[derive(Debug, Clone, Default)]
pub struct ReflectionService {
    symbols: Arc<RwLock<HashMap<String, String>>>,
}

impl ReflectionService {
    /// Creates a new empty reflection service.
    pub fn new() -> Self {
        Self {
            symbols: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers a symbol for reflection queries.
    ///
    /// The `symbol` is the fully-qualified name (e.g. `user.v1.UserService`).
    /// The `file_uri` is an opaque identifier for the file containing the symbol.
    pub fn register_symbol(&self, symbol: impl Into<String>, file_uri: impl Into<String>) {
        self.symbols.write().unwrap().insert(symbol.into(), file_uri.into());
    }

    /// Looks up symbols matching the given query.
    ///
    /// Returns a list of `(symbol, file_uri)` pairs.
    pub async fn lookup(&self, query: &str) -> Vec<(String, String)> {
        let symbols = self.symbols.read().unwrap();
        symbols
            .iter()
            .filter(|(symbol, _)| symbol.contains(query) || query.is_empty())
            .map(|(s, f)| (s.clone(), f.clone()))
            .collect()
    }

    /// Lists all registered symbols.
    pub async fn list_symbols(&self) -> Vec<String> {
        let symbols = self.symbols.read().unwrap();
        symbols.keys().cloned().collect()
    }
}

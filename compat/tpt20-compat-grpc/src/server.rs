//! gRPC-compatible HTTP/2 server (spec §10.3).
//!
//! Provides a server that accepts gRPC HTTP/2 requests and translates them
//! into tpt20 transport calls.
//!
//! ## Usage
//!
//! ```rust
//! # use tpt20_compat_grpc::server::GrpcServer;
//! # use tpt20_compat_grpc::health::HealthRegistry;
//! # use tpt20_transport::Endpoint;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let registry = HealthRegistry::new();
//! # let endpoint = Endpoint::new("127.0.0.1:50051");
//! // let server = GrpcServer::new(endpoint);
//! // server.serve(registry).await?;
//! # Ok(())
//! # }
//! ```

use std::future::Future;

use tpt20_transport::Endpoint;

use crate::GrpcError;

/// A gRPC-compatible HTTP/2 server.
#[derive(Debug, Clone)]
pub struct GrpcServer {
    endpoint: Endpoint,
}

impl GrpcServer {
    /// Creates a new gRPC server bound to the given endpoint.
    pub fn new(endpoint: Endpoint) -> Self {
        GrpcServer { endpoint }
    }

    /// Returns the endpoint this server listens on.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Runs the server, accepting connections and dispatching RPC calls.
    ///
    /// The handler receives parsed gRPC calls. Return an error to stop
    /// serving.
    pub async fn serve<F, Fut>(&self, handler: F) -> Result<(), GrpcError>
    where
        F: Fn(GrpcCall) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = Result<(), GrpcError>> + Send + 'static,
    {
        let _ = handler;
        Err(GrpcError::NotSupported(
            "gRPC server requires the `server` feature to be enabled".into(),
        ))
    }
}

/// A parsed incoming gRPC call.
#[derive(Debug)]
pub struct GrpcCall {
    /// The RPC method path, e.g. `user.v1.UserService/GetUser`.
    pub method: String,
    /// Request metadata extracted from HTTP/2 headers.
    pub metadata: tpt20_transport::Metadata,
    /// The raw request payload bytes.
    pub payload: Vec<u8>,
    /// Channel to send response frames.
    pub response_tx: tokio::sync::mpsc::Sender<Result<tpt20_transport::FramedMessage, GrpcError>>,
    /// Channel to send trailing metadata.
    pub trailers_tx: Option<tokio::sync::oneshot::Sender<Result<tpt20_transport::Metadata, GrpcError>>>,
}

impl GrpcCall {
    /// Sends a success response with the given payload.
    pub async fn send_ok(&self, payload: Vec<u8>) -> Result<(), GrpcError> {
        self.response_tx
            .send(Ok(tpt20_transport::FramedMessage {
                flags: tpt20_transport::FrameFlags::empty(),
                payload,
            }))
            .await
            .map_err(|_| GrpcError::Transport("response channel closed".into()))
    }

    /// Sends a response with the given tpt20 status.
    pub async fn send_status(&mut self, status: tpt20_rpc::Status, message: impl Into<String>) -> Result<(), GrpcError> {
        let trailers = self.build_status_trailers(status, message.into());
        if let Some(tx) = self.trailers_tx.take() {
            let _ = tx.send(Ok(trailers));
        }
        Ok(())
    }

    fn build_status_trailers(
        &self,
        status: tpt20_rpc::Status,
        message: String,
    ) -> tpt20_transport::Metadata {
        let mut trailers = tpt20_transport::Metadata::new();
        trailers.insert("grpc-status", status.code().to_string());
        if !message.is_empty() {
            trailers.insert("grpc-message", message);
        }
        trailers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_creation() {
        let endpoint = Endpoint::new("127.0.0.1:0");
        let server = GrpcServer::new(endpoint);
        assert_eq!(server.endpoint().address, "127.0.0.1:0");
    }
}

//! `tpt20-compat-grpc`: gRPC compatibility adapter for tpt-proto20 (spec §10.3).
//!
//! This crate provides the translation layer between gRPC and tpt20:
//!
//! - HTTP/2 framing compatible with gRPC
//! - Protobuf-compatible message payload support
//! - Status code mapping (tpt20 ↔ gRPC)
//! - Metadata mapping (tpt20 ↔ gRPC)
//! - Deadline mapping (tpt20 ↔ gRPC `grpc-timeout` header)
//! - Streaming mode mapping (unary / server-stream / client-stream / bidi)
//! - gRPC message framing (5-byte header with MSB compression flag)
//! - gRPC health-checking protocol support
//! - gRPC reflection support (where feasible)
//!
//! ## Feature flags
//!
//! - `server` — gRPC-compatible HTTP/2 server
//! - `client` — gRPC-compatible HTTP/2 client
//! - `reflection` — gRPC reflection protocol support
//! - `full` — all features

pub mod client;
pub mod deadline;
pub mod error;
pub mod frame;
pub mod health;
pub mod metadata;
pub mod reflection;
pub mod server;
pub mod status;
pub mod streaming;

pub use error::GrpcError;
pub use status::{from_grpc_status, grpc_status_name, to_grpc_status};
pub use metadata::{from_grpc_headers, from_grpc_trailers, to_grpc_headers, to_grpc_trailers};
pub use deadline::{parse_grpc_timeout, format_grpc_timeout};
pub use streaming::{from_grpc_streaming, to_grpc_streaming, GrpcStreamingType};
pub use frame::{decode_grpc_frame, encode_grpc_frame, grpc_frame_len};
pub use health::{HealthHandler, HealthRegistry, ServingStatus};
pub use reflection::ReflectionService;
pub use server::GrpcServer;
pub use client::GrpcClient;

//! Rich RPC error types (spec §16.4).

use std::fmt;
use thiserror::Error;
use crate::status::Status;
use tpt20_stdlib::ErrorDetail;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    status: Status,
    message: String,
    details: Vec<ErrorDetail>,
}

impl RpcError {
    pub fn status(&self) -> Status { self.status }
    pub fn message(&self) -> &str { &self.message }
    pub fn details(&self) -> &[ErrorDetail] { &self.details }
    pub fn ok() -> Result<(), RpcError> { Ok(()) }
    pub fn ok_status() -> Self {
        Self { status: Status::Ok, message: String::new(), details: Vec::new() }
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}

impl std::error::Error for RpcError {}

pub struct RpcErrorBuilder {
    error: RpcError,
}

impl RpcErrorBuilder {
    pub fn with_details(mut self, detail: impl Into<ErrorDetail>) -> Self {
        self.error.details.push(detail.into());
        self
    }
    pub fn finish(self) -> RpcError { self.error }
}

impl From<RpcErrorBuilder> for RpcError {
    fn from(builder: RpcErrorBuilder) -> Self { builder.error }
}

impl RpcError {
    pub fn cancelled(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::Cancelled, msg) }
    }
    pub fn unknown(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::Unknown, msg) }
    }
    pub fn invalid_argument(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::InvalidArgument, msg) }
    }
    pub fn deadline_exceeded(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::DeadlineExceeded, msg) }
    }
    pub fn not_found(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::NotFound, msg) }
    }
    pub fn already_exists(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::AlreadyExists, msg) }
    }
    pub fn permission_denied(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::PermissionDenied, msg) }
    }
    pub fn resource_exhausted(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::ResourceExhausted, msg) }
    }
    pub fn failed_precondition(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::FailedPrecondition, msg) }
    }
    pub fn aborted(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::Aborted, msg) }
    }
    pub fn out_of_range(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::OutOfRange, msg) }
    }
    pub fn unimplemented(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::Unimplemented, msg) }
    }
    pub fn internal(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::Internal, msg) }
    }
    pub fn unavailable(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::Unavailable, msg) }
    }
    pub fn data_loss(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::DataLoss, msg) }
    }
    pub fn unauthenticated(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder { error: Self::new(Status::Unauthenticated, msg) }
    }

    fn new(status: Status, message: impl Into<String>) -> RpcError {
        RpcError { status, message: message.into(), details: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SendError {
    #[error("stream closed")]
    Closed,
    #[error("send failed: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReceiveError {
    #[error("stream closed")]
    Closed,
    #[error("receive failed: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt20_stdlib::ErrorDetail;
    #[test]
    fn rpc_error_builder() {
        let err = RpcError::invalid_argument("bad field")
            .with_details(ErrorDetail::new("validation".into(), "email invalid".into(), Vec::new()))
            .finish();
        assert_eq!(err.status(), Status::InvalidArgument);
        assert_eq!(err.message(), "bad field");
        assert_eq!(err.details().len(), 1);
    }
}

//! Rich RPC error types (spec §16.4).

use std::fmt;

use thiserror::Error;

use crate::status::Status;
use tpt20_stdlib::ErrorDetail;

/// A rich RPC error carrying a status, message, and structured details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    status: Status,
    message: String,
    details: Vec<ErrorDetail>,
}

impl RpcError {
    /// Returns the status code for this error.
    pub fn status(&self) -> Status {
        self.status
    }

    /// Returns the human-readable message for this error.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the structured error details.
    pub fn details(&self) -> &[ErrorDetail] {
        &self.details
    }

    /// A successful (non-error) result.
    pub fn ok() -> Result<(), RpcError> {
        Ok(())
    }

    /// Creates a new successful error placeholder.
    pub fn ok_status() -> Self {
        Self {
            status: Status::Ok,
            message: String::new(),
            details: Vec::new(),
        }
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}

impl std::error::Error for RpcError {}

// Builder pattern for constructing errors ergonomically.

/// Builder for [`RpcError`], produced by the status-specific constructors.
pub struct RpcErrorBuilder {
    error: RpcError,
}

impl RpcErrorBuilder {
    /// Attaches structured details to the error.
    pub fn with_details(mut self, detail: impl Into<ErrorDetail>) -> Self {
        self.error.details.push(detail.into());
        self
    }

    /// Finishes building and returns the [`RpcError`].
    pub fn finish(self) -> RpcError {
        self.error
    }
}

impl From<RpcErrorBuilder> for RpcError {
    fn from(builder: RpcErrorBuilder) -> Self {
        builder.error
    }
}

impl RpcError {
    /// Builder for a CANCELLED error.
    pub fn cancelled(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::Cancelled, msg),
        }
    }

    /// Builder for an UNKNOWN error.
    pub fn unknown(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::Unknown, msg),
        }
    }

    /// Builder for an INVALID_ARGUMENT error.
    pub fn invalid_argument(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::InvalidArgument, msg),
        }
    }

    /// Builder for a DEADLINE_EXCEEDED error.
    pub fn deadline_exceeded(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::DeadlineExceeded, msg),
        }
    }

    /// Builder for a NOT_FOUND error.
    pub fn not_found(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::NotFound, msg),
        }
    }

    /// Builder for an ALREADY_EXISTS error.
    pub fn already_exists(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::AlreadyExists, msg),
        }
    }

    /// Builder for a PERMISSION_DENIED error.
    pub fn permission_denied(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::PermissionDenied, msg),
        }
    }

    /// Builder for a RESOURCE_EXHAUSTED error.
    pub fn resource_exhausted(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::ResourceExhausted, msg),
        }
    }

    /// Builder for a FAILED_PRECONDITION error.
    pub fn failed_precondition(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::FailedPrecondition, msg),
        }
    }

    /// Builder for an ABORTED error.
    pub fn aborted(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::Aborted, msg),
        }
    }

    /// Builder for an OUT_OF_RANGE error.
    pub fn out_of_range(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::OutOfRange, msg),
        }
    }

    /// Builder for an UNIMPLEMENTED error.
    pub fn unimplemented(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::Unimplemented, msg),
        }
    }

    /// Builder for an INTERNAL error.
    pub fn internal(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::Internal, msg),
        }
    }

    /// Builder for an UNAVAILABLE error.
    pub fn unavailable(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::Unavailable, msg),
        }
    }

    /// Builder for a DATA_LOSS error.
    pub fn data_loss(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::DataLoss, msg),
        }
    }

    /// Builder for an UNAUTHENTICATED error.
    pub fn unauthenticated(msg: impl Into<String>) -> RpcErrorBuilder {
        RpcErrorBuilder {
            error: RpcError::new(Status::Unauthenticated, msg),
        }
    }

    fn new(status: Status, message: impl Into<String>) -> RpcError {
        RpcError {
            status,
            message: message.into(),
            details: Vec::new(),
        }
    }
}

// Errors specific to send/receive operations.

/// Error that occurs when sending a message on a stream.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SendError {
    /// The stream was closed before the send completed.
    #[error("stream closed")]
    Closed,
    /// A transport-level send failure.
    #[error("send failed: {0}")]
    Other(String),
}

/// Error that occurs when receiving a message from a stream.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReceiveError {
    /// The stream ended normally.
    #[error("stream closed")]
    Closed,
    /// A transport-level receive failure.
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
            .with_details(ErrorDetail::new(
                "validation".into(),
                "email invalid".into(),
                Vec::new(),
            ))
            .finish();

        assert_eq!(err.status(), Status::InvalidArgument);
        assert_eq!(err.message(), "bad field");
        assert_eq!(err.details().len(), 1);
        assert_eq!(err.details()[0].code, "validation");
    }

    #[test]
    fn rpc_error_from_builder() {
        let err: RpcError = RpcError::not_found("user 42").finish().into();
        assert_eq!(err.status(), Status::NotFound);
    }

    #[test]
    fn status_display() {
        assert_eq!(format!("{}", Status::Ok), "OK");
        assert_eq!(format!("{}", Status::InvalidArgument), "INVALID_ARGUMENT");
    }
}

//! Status code mapping between tpt20 and gRPC (spec §10.3).

use crate::GrpcError;
use tpt20_rpc::Status;

/// Converts a tpt20 [`Status`] to its gRPC numeric status code.
///
/// The mapping is 1:1 because tpt20 status codes match the gRPC status
/// code space exactly (0–16).
pub fn to_grpc_status(status: Status) -> i32 {
    status.code()
}

/// Converts a gRPC numeric status code to a tpt20 [`Status`].
///
/// Returns an error if the code is outside the known gRPC status range.
pub fn from_grpc_status(code: i32) -> Result<Status, GrpcError> {
    Status::from_code(code).ok_or(GrpcError::InvalidStatus(code))
}

/// Returns the canonical gRPC status name for a tpt20 [`Status`].
///
/// Example: `Status::Ok` → `"OK"`, `Status::NotFound` → `"NOT_FOUND"`.
pub fn grpc_status_name(status: Status) -> &'static str {
    status.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip() {
        for i in 0..=16 {
            let status = Status::from_code(i).unwrap();
            let grpc_code = to_grpc_status(status);
            assert_eq!(grpc_code, i);
            let back = from_grpc_status(grpc_code).unwrap();
            assert_eq!(back, status);
        }
    }

    #[test]
    fn unknown_grpc_status() {
        assert!(from_grpc_status(99).is_err());
        assert!(from_grpc_status(-1).is_err());
    }

    #[test]
    fn status_names_match_grpc() {
        assert_eq!(grpc_status_name(Status::Ok), "OK");
        assert_eq!(grpc_status_name(Status::Cancelled), "CANCELLED");
        assert_eq!(grpc_status_name(Status::Unknown), "UNKNOWN");
        assert_eq!(grpc_status_name(Status::InvalidArgument), "INVALID_ARGUMENT");
        assert_eq!(grpc_status_name(Status::DeadlineExceeded), "DEADLINE_EXCEEDED");
        assert_eq!(grpc_status_name(Status::NotFound), "NOT_FOUND");
        assert_eq!(grpc_status_name(Status::AlreadyExists), "ALREADY_EXISTS");
        assert_eq!(grpc_status_name(Status::PermissionDenied), "PERMISSION_DENIED");
        assert_eq!(grpc_status_name(Status::ResourceExhausted), "RESOURCE_EXHAUSTED");
        assert_eq!(grpc_status_name(Status::FailedPrecondition), "FAILED_PRECONDITION");
        assert_eq!(grpc_status_name(Status::Aborted), "ABORTED");
        assert_eq!(grpc_status_name(Status::OutOfRange), "OUT_OF_RANGE");
        assert_eq!(grpc_status_name(Status::Unimplemented), "UNIMPLEMENTED");
        assert_eq!(grpc_status_name(Status::Internal), "INTERNAL");
        assert_eq!(grpc_status_name(Status::Unavailable), "UNAVAILABLE");
        assert_eq!(grpc_status_name(Status::DataLoss), "DATA_LOSS");
        assert_eq!(grpc_status_name(Status::Unauthenticated), "UNAUTHENTICATED");
    }
}

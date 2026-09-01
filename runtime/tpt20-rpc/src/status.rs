//! RPC status codes (spec §16.3).

use thiserror::Error;

/// Standard RPC status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    Cancelled = 1,
    Unknown = 2,
    InvalidArgument = 3,
    DeadlineExceeded = 4,
    NotFound = 5,
    AlreadyExists = 6,
    PermissionDenied = 7,
    ResourceExhausted = 8,
    FailedPrecondition = 9,
    Aborted = 10,
    OutOfRange = 11,
    Unimplemented = 12,
    Internal = 13,
    Unavailable = 14,
    DataLoss = 15,
    Unauthenticated = 16,
}

impl Status {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Cancelled => "CANCELLED",
            Status::Unknown => "UNKNOWN",
            Status::InvalidArgument => "INVALID_ARGUMENT",
            Status::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Status::NotFound => "NOT_FOUND",
            Status::AlreadyExists => "ALREADY_EXISTS",
            Status::PermissionDenied => "PERMISSION_DENIED",
            Status::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Status::FailedPrecondition => "FAILED_PRECONDITION",
            Status::Aborted => "ABORTED",
            Status::OutOfRange => "OUT_OF_RANGE",
            Status::Unimplemented => "UNIMPLEMENTED",
            Status::Internal => "INTERNAL",
            Status::Unavailable => "UNAVAILABLE",
            Status::DataLoss => "DATA_LOSS",
            Status::Unauthenticated => "UNAUTHENTICATED",
        }
    }

    pub const fn code(&self) -> i32 { *self as i32 }

    pub fn from_code(code: i32) -> Option<Status> {
        match code {
            0 => Some(Status::Ok), 1 => Some(Status::Cancelled), 2 => Some(Status::Unknown),
            3 => Some(Status::InvalidArgument), 4 => Some(Status::DeadlineExceeded),
            5 => Some(Status::NotFound), 6 => Some(Status::AlreadyExists),
            7 => Some(Status::PermissionDenied), 8 => Some(Status::ResourceExhausted),
            9 => Some(Status::FailedPrecondition), 10 => Some(Status::Aborted),
            11 => Some(Status::OutOfRange), 12 => Some(Status::Unimplemented),
            13 => Some(Status::Internal), 14 => Some(Status::Unavailable),
            15 => Some(Status::DataLoss), 16 => Some(Status::Unauthenticated),
            _ => None,
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<Status> for i32 {
    fn from(status: Status) -> i32 { status.code() }
}

impl TryFrom<i32> for Status {
    type Error = UnknownStatusCode;
    fn try_from(code: i32) -> Result<Self, Self::Error> {
        Status::from_code(code).ok_or(UnknownStatusCode(code))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("unknown RPC status code: {0}")]
pub struct UnknownStatusCode(pub i32);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_code() {
        for i in 0..=16 {
            let s = Status::from_code(i).unwrap();
            assert_eq!(i32::from(s), i);
            assert_eq!(Status::try_from(i).unwrap(), s);
        }
    }
    #[test]
    fn status_display() {
        assert_eq!(format!("{}", Status::Ok), "OK");
    }
}

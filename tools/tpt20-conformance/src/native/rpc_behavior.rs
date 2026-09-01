use tpt20_rpc::{RpcContext, Status, RpcError};

#[test]
fn status_codes_roundtrip() {
    for i in 0..=16 {
        let s = Status::from_code(i).unwrap();
        assert_eq!(i32::from(s), i);
        assert_eq!(Status::try_from(i).unwrap(), s);
    }
}

#[test]
fn status_display() {
    assert_eq!(format!("{}", Status::Ok), "OK");
    assert_eq!(format!("{}", Status::NotFound), "NOT_FOUND");
}

#[test]
fn status_from_code_invalid() {
    assert!(Status::from_code(99).is_none());
    assert!(Status::try_from(99).is_err());
}

#[test]
fn rpc_error_builder() {
    let err = RpcError::invalid_argument("bad field")
        .with_details(tpt20_stdlib::ErrorDetail::new("validation".into(), "email invalid".into(), Vec::new()))
        .finish();
    assert_eq!(err.status(), Status::InvalidArgument);
    assert_eq!(err.message(), "bad field");
    assert_eq!(err.details().len(), 1);
}

#[test]
fn rpc_context_default() {
    let ctx = RpcContext::new();
    assert!(!ctx.is_expired());
    assert!(ctx.peer().is_none());
    assert!(ctx.extensions().is_empty());
}

#[test]
fn rpc_context_with_deadline() {
    use std::time::Duration;
    let ctx = RpcContext::new()
        .with_deadline(tpt20_rpc::Deadline::from_now(Duration::from_secs(5)))
        .with_trace(tpt20_rpc::TraceContext::new("t1", "s1", 1))
        .with_peer(tpt20_rpc::PeerInfo::new("127.0.0.1", 9090));
    assert!(!ctx.is_expired());
    assert_eq!(ctx.trace().trace_id, "t1");
    assert_eq!(ctx.peer().unwrap().addr, "127.0.0.1");
}

#[test]
fn rpc_context_metadata() {
    let mut ctx = RpcContext::new();
    ctx.metadata_mut().insert_text("x-key", "value").unwrap();
    assert_eq!(ctx.metadata().get_first_text("x-key"), Some("value"));
}

#[test]
fn all_status_codes_have_names() {
    let names = [
        ("OK", Status::Ok),
        ("CANCELLED", Status::Cancelled),
        ("UNKNOWN", Status::Unknown),
        ("INVALID_ARGUMENT", Status::InvalidArgument),
        ("DEADLINE_EXCEEDED", Status::DeadlineExceeded),
        ("NOT_FOUND", Status::NotFound),
        ("ALREADY_EXISTS", Status::AlreadyExists),
        ("PERMISSION_DENIED", Status::PermissionDenied),
        ("RESOURCE_EXHAUSTED", Status::ResourceExhausted),
        ("FAILED_PRECONDITION", Status::FailedPrecondition),
        ("ABORTED", Status::Aborted),
        ("OUT_OF_RANGE", Status::OutOfRange),
        ("UNIMPLEMENTED", Status::Unimplemented),
        ("INTERNAL", Status::Internal),
        ("UNAVAILABLE", Status::Unavailable),
        ("DATA_LOSS", Status::DataLoss),
        ("UNAUTHENTICATED", Status::Unauthenticated),
    ];
    for (expected, status) in names {
        assert_eq!(status.as_str(), expected);
    }
}

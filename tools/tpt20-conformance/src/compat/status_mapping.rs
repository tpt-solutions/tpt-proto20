use tpt20_compat_grpc::{from_grpc_status, to_grpc_status};
use tpt20_rpc::Status;

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
fn all_status_codes_map() {
    for i in 0..=16 {
        let status = Status::from_code(i).unwrap();
        let code = to_grpc_status(status);
        assert_eq!(code, i);
    }
}

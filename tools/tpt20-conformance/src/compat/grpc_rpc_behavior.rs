use tpt20_compat_grpc::{decode_grpc_frame, encode_grpc_frame, grpc_frame_len};

#[test]
fn decode_uncompressed_grpc_frame() {
    let payload = b"hello world";
    let mut frame = vec![0x00, 0x00, 0x00, 0x00, 0x0b];
    frame.extend_from_slice(payload);
    let (flags, decoded) = decode_grpc_frame(&frame).unwrap();
    assert!(!flags.is_compressed());
    assert_eq!(decoded, payload);
}

#[test]
fn decode_compressed_grpc_frame() {
    let payload = b"compressed";
    let mut frame = vec![0x80, 0x00, 0x00, 0x00, 0x0a];
    frame.extend_from_slice(payload);
    let (flags, decoded) = decode_grpc_frame(&frame).unwrap();
    assert!(flags.is_compressed());
    assert_eq!(decoded, payload);
}

#[test]
fn encode_uncompressed_grpc_frame() {
    let payload = b"hello";
    let frame = encode_grpc_frame(payload, false).unwrap();
    assert_eq!(&frame[0..5], &[0x00, 0x00, 0x00, 0x00, 0x05]);
    assert_eq!(&frame[5..], payload);
}

#[test]
fn encode_compressed_grpc_frame() {
    let payload = b"hello";
    let frame = encode_grpc_frame(payload, true).unwrap();
    assert_eq!(&frame[0..5], &[0x80, 0x00, 0x00, 0x00, 0x05]);
    assert_eq!(&frame[5..], payload);
}

#[test]
fn grpc_frame_len_constant() {
    assert_eq!(grpc_frame_len(0), 5);
    assert_eq!(grpc_frame_len(100), 105);
}

#[test]
fn decode_rejects_reserved_bits() {
    let mut frame = vec![0x7f, 0x00, 0x00, 0x00, 0x00];
    frame.extend_from_slice(b"x");
    assert!(decode_grpc_frame(&frame).is_err());
}

#[test]
fn decode_rejects_truncated() {
    assert!(decode_grpc_frame(b"").is_err());
    assert!(decode_grpc_frame(b"\x00\x00\x00").is_err());
    assert!(decode_grpc_frame(b"\x00\x00\x00\x00\x05ab").is_err());
}

#[test]
fn frame_flags_conversion() {
    let frame_bytes = encode_grpc_frame(b"test", true).unwrap();
    let (flags, _) = decode_grpc_frame(&frame_bytes).unwrap();
    assert!(flags.is_compressed());
}

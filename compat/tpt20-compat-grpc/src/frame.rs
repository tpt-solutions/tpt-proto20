//! gRPC message framing (spec §10.3).
//!
//! gRPC messages over HTTP/2 are length-prefixed with a 5-byte header:
//!
//! ```text
//! 1 byte:  MSB = compression flag, bits 0-6 = reserved (must be 0)
//! 4 bytes: big-endian message length
//! N bytes: payload
//! ```
//!
//! This module encodes and decodes gRPC frames and translates between gRPC
//! framing and tpt20 framing (which uses bit 0 for compression instead of
//! bit 7).

use crate::GrpcError;
use tpt20_transport::FrameFlags;

/// gRPC compression flag mask (MSB of first byte).
pub const GRPC_COMPRESSED_MASK: u8 = 0x80;

/// Reserved bits mask (bits 0-6 must be zero in gRPC frames).
pub const GRPC_RESERVED_MASK: u8 = 0x7F;

/// Decodes a gRPC message frame from bytes.
///
/// Returns the tpt20 [`FrameFlags`] and the payload slice. The returned
/// payload is a borrow into the input buffer.
///
/// # Errors
///
/// Returns [`GrpcError::InvalidFrame`] if the buffer is too short or the
/// length prefix does not match the actual payload size.
pub fn decode_grpc_frame(bytes: &[u8]) -> Result<(FrameFlags, &[u8]), GrpcError> {
    if bytes.len() < 5 {
        return Err(GrpcError::InvalidFrame(
            "frame too short (header)".into(),
        ));
    }
    let flags_byte = bytes[0];
    if (flags_byte & GRPC_RESERVED_MASK) != 0 {
        return Err(GrpcError::InvalidFrame(format!(
            "reserved gRPC frame flags bits set: 0x{:02x}",
            flags_byte & GRPC_RESERVED_MASK
        )));
    }
    let length = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    if bytes.len() < 5 + length {
        return Err(GrpcError::InvalidFrame(
            "frame too short (payload)".into(),
        ));
    }
    let payload = &bytes[5..5 + length];
    let compressed = (flags_byte & GRPC_COMPRESSED_MASK) != 0;
    let flags = if compressed {
        FrameFlags::compressed()
    } else {
        FrameFlags::empty()
    };
    Ok((flags, payload))
}

/// Encodes a payload as a gRPC message frame.
///
/// # Errors
///
/// Returns [`GrpcError::InvalidFrame`] if the payload length exceeds
/// `u32::MAX`.
pub fn encode_grpc_frame(payload: &[u8], compressed: bool) -> Result<Vec<u8>, GrpcError> {
    if payload.len() > u32::MAX as usize {
        return Err(GrpcError::InvalidFrame(
            "payload too large for gRPC frame".into(),
        ));
    }
    let mut buf = Vec::with_capacity(5 + payload.len());
    if compressed {
        buf.push(GRPC_COMPRESSED_MASK);
    } else {
        buf.push(0);
    }
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);
    Ok(buf)
}

/// Returns the encoded length of a gRPC frame for a payload of `payload_len`
/// bytes.
pub const fn grpc_frame_len(payload_len: usize) -> usize {
    5 + payload_len
}

/// Converts a gRPC-framed message to a tpt20 [`FrameFlags`] and payload.
///
/// This is a convenience wrapper around [`decode_grpc_frame`] that returns
/// an owned payload.
pub fn grpc_frame_to_parts(bytes: &[u8]) -> Result<(FrameFlags, Vec<u8>), GrpcError> {
    let (flags, payload) = decode_grpc_frame(bytes)?;
    Ok((flags, payload.to_vec()))
}

/// Converts a payload to a gRPC-framed [`Vec<u8>`].
pub fn parts_to_grpc_frame(flags: FrameFlags, payload: &[u8]) -> Result<Vec<u8>, GrpcError> {
    encode_grpc_frame(payload, flags.is_compressed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_uncompressed() {
        let payload = b"hello world";
        let mut frame = vec![0x00, 0x00, 0x00, 0x00, 0x0b];
        frame.extend_from_slice(payload);
        let (flags, decoded) = decode_grpc_frame(&frame).unwrap();
        assert!(!flags.is_compressed());
        assert_eq!(decoded, payload);
    }

    #[test]
    fn decode_compressed() {
        let payload = b"compressed";
        let mut frame = vec![0x80, 0x00, 0x00, 0x00, 0x0a];
        frame.extend_from_slice(payload);
        let (flags, decoded) = decode_grpc_frame(&frame).unwrap();
        assert!(flags.is_compressed());
        assert_eq!(decoded, payload);
    }

    #[test]
    fn encode_uncompressed() {
        let payload = b"hello";
        let frame = encode_grpc_frame(payload, false).unwrap();
        assert_eq!(&frame[0..5], &[0x00, 0x00, 0x00, 0x00, 0x05]);
        assert_eq!(&frame[5..], payload);
    }

    #[test]
    fn encode_compressed() {
        let payload = b"hello";
        let frame = encode_grpc_frame(payload, true).unwrap();
        assert_eq!(&frame[0..5], &[0x80, 0x00, 0x00, 0x00, 0x05]);
        assert_eq!(&frame[5..], payload);
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
    fn roundtrip() {
        let payload = b"roundtrip data";
        let frame = encode_grpc_frame(payload, false).unwrap();
        let (flags, decoded) = decode_grpc_frame(&frame).unwrap();
        assert_eq!(decoded, payload);
        assert!(!flags.is_compressed());
    }

    #[test]
    fn test_grpc_frame_len() {
        assert_eq!(grpc_frame_len(0), 5);
        assert_eq!(grpc_frame_len(100), 105);
    }
}

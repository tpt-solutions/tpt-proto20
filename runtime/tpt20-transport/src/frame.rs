//! Message framing for the tpt20 transport (spec §17.2).
//!
//! Every RPC message on the wire is framed as:
//!
//! ```text
//! 1 byte flags
//! 4 bytes big-endian length
//! N bytes payload
//! ```
//!
//! ### Flags byte layout
//!
//! | Bit | Mask | Meaning |
//! |-----|------|---------|
//! |  0  | 0x01 | Compression enabled (payload is compressed) |
//! |  1-7| 0xFE | Reserved for future protocol extensions (must be 0) |

use crate::error::TransportError;

/// Flags for the tpt20 message frame header (spec §17.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameFlags(u8);

impl FrameFlags {
    /// Flag mask: compression enabled.
    pub const COMPRESSED: u8 = 0x01;

    /// Mask for reserved bits (bits 1-7).
    pub const RESERVED_MASK: u8 = 0xFE;

    /// Creates empty flags (no compression, reserved bits cleared).
    pub const fn empty() -> Self {
        FrameFlags(0)
    }

    /// Creates flags with the compressed bit set.
    pub const fn compressed() -> Self {
        FrameFlags(Self::COMPRESSED)
    }

    /// Returns true if the compressed flag is set.
    pub fn is_compressed(&self) -> bool {
        (self.0 & Self::COMPRESSED) != 0
    }

    /// Returns the raw byte value.
    pub const fn raw(&self) -> u8 {
        self.0
    }

    /// Creates flags from a raw byte, validating reserved bits.
    pub fn from_raw(raw: u8) -> Result<Self, TransportError> {
        if (raw & Self::RESERVED_MASK) != 0 {
            return Err(TransportError::MalformedFrame(format!(
                "reserved frame flags bits set: 0x{:02x}",
                raw & Self::RESERVED_MASK
            )));
        }
        Ok(FrameFlags(raw))
    }

    /// Sets the compressed flag.
    pub fn set_compressed(mut self, compressed: bool) -> Self {
        if compressed {
            self.0 |= Self::COMPRESSED;
        } else {
            self.0 &= !Self::COMPRESSED;
        }
        self
    }
}

impl Default for FrameFlags {
    fn default() -> Self {
        Self::empty()
    }
}

/// A complete tpt20 message frame on the wire.
///
/// Layout (spec §17.2):
/// - 1 byte flags
/// - 4 bytes big-endian length
/// - N bytes payload
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Frame header flags.
    pub flags: FrameFlags,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

/// A framed message used internally by transports.
#[derive(Debug, Clone)]
pub struct FramedMessage {
    /// Frame header flags.
    pub flags: FrameFlags,
    /// The payload bytes.
    pub payload: Vec<u8>,
}

impl FramedMessage {
    /// Creates a new framed message.
    pub fn new(flags: FrameFlags, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            flags,
            payload: payload.into(),
        }
    }

    /// Creates an empty framed message (no flags, empty payload).
    pub fn empty() -> Self {
        Self {
            flags: FrameFlags::empty(),
            payload: Vec::new(),
        }
    }
}

impl Frame {
    /// Encodes this frame into bytes.
    ///
    /// Layout: `[flags: 1][length: 4 big-endian][payload: N]`
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(5 + self.payload.len());
        buf.push(self.flags.raw());
        buf.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Encodes a payload as a frame with the given flags.
    pub fn encode_with(payload: &[u8], flags: FrameFlags) -> Vec<u8> {
        Frame {
            flags,
            payload: payload.to_vec(),
        }
        .encode()
    }

    /// Decodes a frame from bytes.
    ///
    /// Returns the frame and the number of bytes consumed.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), TransportError> {
        if bytes.len() < 5 {
            return Err(TransportError::MalformedFrame(
                "frame too short (header)".into(),
            ));
        }

        let flags = FrameFlags::from_raw(bytes[0])?;
        let length = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;

        if bytes.len() < 5 + length {
            return Err(TransportError::MalformedFrame(
                "frame too short (payload)".into(),
            ));
        }

        let payload = bytes[5..5 + length].to_vec();
        Ok((
            Frame {
                flags,
                payload,
            },
            5 + length,
        ))
    }

    /// Returns the encoded frame length without the payload.
    pub const fn header_len() -> usize {
        5
    }

    /// Returns the total encoded frame length.
    pub fn encoded_len(&self) -> usize {
        5 + self.payload.len()
    }
}

/// Encodes a message payload as a tpt20 framed message.
pub fn encode_frame(payload: &[u8], compressed: bool) -> Vec<u8> {
    let flags = FrameFlags::empty().set_compressed(compressed);
    Frame::encode_with(payload, flags)
}

/// Decodes a tpt20 framed message.
///
/// Returns the frame and the number of bytes consumed from `buf`.
pub fn decode_frame(buf: &[u8]) -> Result<(Frame, usize), TransportError> {
    Frame::decode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip_uncompressed() {
        let payload = b"hello world";
        let frame_bytes = encode_frame(payload, false);
        let (frame, n) = decode_frame(&frame_bytes).unwrap();
        assert_eq!(n, frame_bytes.len());
        assert!(!frame.flags.is_compressed());
        assert_eq!(frame.payload, payload);
    }

    #[test]
    fn frame_roundtrip_compressed() {
        let payload = b"compressed data";
        let frame_bytes = encode_frame(payload, true);
        let (frame, n) = decode_frame(&frame_bytes).unwrap();
        assert_eq!(n, frame_bytes.len());
        assert!(frame.flags.is_compressed());
        assert_eq!(frame.payload, payload);
    }

    #[test]
    fn frame_rejects_reserved_bits() {
        let mut bytes = vec![0xFE, 0, 0, 0, 0];
        assert!(Frame::decode(&bytes).is_err());
    }

    #[test]
    fn frame_rejects_truncated() {
        assert!(Frame::decode(b"").is_err());
        assert!(Frame::decode(b"\x00").is_err());
        assert!(Frame::decode(b"\x00\x00\x00\x00\x05").is_err());
    }

    #[test]
    fn empty_payload() {
        let frame_bytes = encode_frame(b"", false);
        let (frame, n) = decode_frame(&frame_bytes).unwrap();
        assert_eq!(n, frame_bytes.len());
        assert!(frame.payload.is_empty());
    }
}

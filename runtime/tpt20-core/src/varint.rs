//! Varint encoding/decoding (7-bit groups, little-endian groups) per spec §9.
//!
//! Includes zigzag encoding for signed integers and strict validation that
//! rejects truncated, overlong, and overflowing varints.

use crate::error::DecodeError;

/// Maximum number of bytes a 64-bit varint may occupy.
const MAX_VARINT_LEN: usize = 10;

/// Encodes `value` as a varint into `out`.
///
/// The most significant bit of each byte is a continuation flag; the lower
/// 7 bits carry data, least-significant group first.
pub fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            out.push(byte | 0x80);
        } else {
            out.push(byte);
            break;
        }
    }
}

/// Encodes `value` as a varint and returns it as a fresh `Vec`.
pub fn encode_varint_vec(value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    encode_varint(value, &mut out);
    out
}

/// Decodes a varint from `bytes` starting at `cursor`.
///
/// Returns the decoded value and the new cursor position.
///
/// # Errors
/// Rejects truncated input (`Truncated`), overlong/overflowing varints
/// (`VarintOverflow`), and invalid payloads.
pub fn decode_varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, DecodeError> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    let mut count = 0;
    loop {
        if *cursor >= bytes.len() {
            return Err(DecodeError::Truncated);
        }
        let byte = bytes[*cursor];
        *cursor += 1;
        count += 1;
        if count > MAX_VARINT_LEN {
            return Err(DecodeError::VarintOverflow);
        }
        let payload = (byte & 0x7f) as u64;
        // Guard against shift overflow on the final group.
        if shift >= 64 {
            return Err(DecodeError::VarintOverflow);
        }
        result |= payload << shift;
        if byte & 0x80 == 0 {
            // For a 64-bit value, the top group may only contribute 1 bit.
            if count == MAX_VARINT_LEN && (byte & 0x7f) > 0x01 {
                return Err(DecodeError::VarintOverflow);
            }
            return Ok(result);
        }
        shift += 7;
    }
}

/// Zigzag-encodes a signed integer to a unsigned varint payload.
#[inline]
pub fn encode_zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// Zigzag-decodes an unsigned varint payload to a signed integer.
#[inline]
pub fn decode_zigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_varint() {
        for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let enc = encode_varint_vec(v);
            let mut cur = 0;
            let dec = decode_varint(&enc, &mut cur).unwrap();
            assert_eq!(dec, v);
            assert_eq!(cur, enc.len());
        }
    }

    #[test]
    fn rejects_truncated() {
        let mut cur = 0;
        assert_eq!(
            decode_varint(&[0x80, 0x80], &mut cur),
            Err(DecodeError::Truncated)
        );
    }

    #[test]
    fn rejects_overflow() {
        // 11 continuation bytes is never valid for a 64-bit varint.
        let bytes = [0x80u8; 11];
        let mut cur = 0;
        assert_eq!(
            decode_varint(&bytes, &mut cur),
            Err(DecodeError::VarintOverflow)
        );
    }

    #[test]
    fn rejects_overlong_top_group() {
        // 10 bytes where the final group has 2 bits set => overflow.
        let bytes = [0x80u8, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02];
        let mut cur = 0;
        assert_eq!(
            decode_varint(&bytes, &mut cur),
            Err(DecodeError::VarintOverflow)
        );
    }

    #[test]
    fn zigzag_roundtrip() {
        for v in [i64::MIN, -2, -1, 0, 1, 2, i64::MAX] {
            assert_eq!(decode_zigzag(encode_zigzag(v)), v);
        }
    }
}

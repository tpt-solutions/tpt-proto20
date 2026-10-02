//! Message compression (spec §16/§17): gzip and deflate.
//!
//! Negotiation follows the gRPC convention: a sender announces the codec it
//! used in `grpc-encoding` and the codecs it can read in
//! `grpc-accept-encoding`. Individual messages are compressed only when they
//! reach [`Endpoint::compression_min_bytes`](crate::Endpoint) and the result
//! is actually smaller; the frame's compressed flag says which messages were.
//! Decompression is bounded, so a small compressed message cannot expand
//! past the receiver's message-size limit (decompression-bomb protection).

/// A supported message compression algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// gzip (RFC 1952).
    Gzip,
    /// zlib-wrapped deflate (RFC 1950), announced as `deflate`.
    Deflate,
}

impl Compression {
    /// The name used in `grpc-encoding` / `grpc-accept-encoding`.
    pub const fn name(self) -> &'static str {
        match self {
            Compression::Gzip => "gzip",
            Compression::Deflate => "deflate",
        }
    }

    /// Parses an encoding name; `identity` and unknown names yield `None`.
    pub fn from_name(name: &str) -> Option<Compression> {
        match name.trim().to_ascii_lowercase().as_str() {
            "gzip" => Some(Compression::Gzip),
            "deflate" => Some(Compression::Deflate),
            _ => None,
        }
    }

    /// Every supported algorithm, in preference order.
    pub const ALL: [Compression; 2] = [Compression::Gzip, Compression::Deflate];

    /// The `grpc-accept-encoding` value advertising every supported codec.
    pub fn accept_header() -> &'static str {
        "gzip,deflate,identity"
    }

    /// Parses a `grpc-accept-encoding` list.
    pub fn parse_accept(list: &str) -> Vec<Compression> {
        list.split(',').filter_map(Compression::from_name).collect()
    }
}

#[cfg(feature = "http2")]
pub(crate) use codec::{compress, decompress};

#[cfg(feature = "http2")]
mod codec {
    use super::Compression;
    use crate::error::TransportError;
    use flate2::read::{GzDecoder, ZlibDecoder};
    use flate2::write::{GzEncoder, ZlibEncoder};
    use std::io::{Read, Write};

    /// Compresses `data`.
    pub(crate) fn compress(alg: Compression, data: &[u8]) -> Vec<u8> {
        let level = flate2::Compression::fast();
        // Writing into a `Vec` cannot fail.
        match alg {
            Compression::Gzip => {
                let mut e = GzEncoder::new(Vec::with_capacity(data.len() / 2 + 32), level);
                e.write_all(data).expect("vec write");
                e.finish().expect("vec write")
            }
            Compression::Deflate => {
                let mut e = ZlibEncoder::new(Vec::with_capacity(data.len() / 2 + 32), level);
                e.write_all(data).expect("vec write");
                e.finish().expect("vec write")
            }
        }
    }

    /// Decompresses `data`, refusing output larger than `max` bytes.
    pub(crate) fn decompress(
        alg: Compression,
        data: &[u8],
        max: usize,
    ) -> Result<Vec<u8>, TransportError> {
        let mut out = Vec::new();
        let limit = (max as u64).saturating_add(1);
        let read = match alg {
            Compression::Gzip => GzDecoder::new(data).take(limit).read_to_end(&mut out),
            Compression::Deflate => ZlibDecoder::new(data).take(limit).read_to_end(&mut out),
        };
        read.map_err(|e| {
            TransportError::Compression(format!("{} decode failed: {e}", alg.name()))
        })?;
        if out.len() > max {
            return Err(TransportError::SizeLimitExceeded { limit: max });
        }
        Ok(out)
    }
}

#[cfg(all(test, feature = "http2"))]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_shrinks_repetitive_data() {
        let data = vec![b'a'; 10_000];
        for alg in Compression::ALL {
            let c = compress(alg, &data);
            assert!(c.len() < 200, "{alg:?}: {}", c.len());
            assert_eq!(decompress(alg, &c, 10_000).unwrap(), data);
        }
    }

    #[test]
    fn decompression_is_bounded() {
        let bomb = compress(Compression::Gzip, &vec![0u8; 5_000_000]);
        assert!(bomb.len() < 100_000);
        assert!(matches!(
            decompress(Compression::Gzip, &bomb, 1 << 20),
            Err(crate::TransportError::SizeLimitExceeded { .. })
        ));
        // Exactly at the limit is fine.
        let ok = compress(Compression::Deflate, &[7u8; 1024]);
        assert_eq!(
            decompress(Compression::Deflate, &ok, 1024).unwrap().len(),
            1024
        );
    }

    #[test]
    fn corrupt_input_is_an_error_and_names_parse() {
        assert!(decompress(Compression::Gzip, b"not gzip", 100).is_err());
        assert!(decompress(Compression::Deflate, b"\x00\x01\x02", 100).is_err());
        assert_eq!(Compression::from_name(" GZIP "), Some(Compression::Gzip));
        assert_eq!(Compression::from_name("identity"), None);
        assert_eq!(
            Compression::parse_accept("br, deflate,identity"),
            vec![Compression::Deflate]
        );
    }
}

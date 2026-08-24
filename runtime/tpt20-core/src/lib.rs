//! `tpt20-core`: native binary wire format and core runtime (spec §9, §11, §18).
//!
//! This crate provides the safe-by-default decode/encode primitives for the
//! tpt20 native wire format. It is deliberately free of `unsafe` in the
//! decoding paths (spec §9 policy) and uses checked arithmetic throughout.
//!
//! The design targets untrusted input: every decoder limit in
//! [`DecoderLimits`] is enforced on the decode path with conservative defaults.

pub mod dynamic;
pub mod error;
pub mod limits;
pub mod message;
pub mod scalar;
pub mod varint;
pub mod wire;

pub use error::{DecodeError, EncodeError};
pub use limits::{DecoderLimits, UnknownFieldPolicy};
pub use message::{split_len_delimited, Field, RawMessage, Value};
pub use wire::{Tag, WireClass};

/// Optional envelope wrapping a schema-identified payload (spec §9).
///
/// This is not required for normal RPC payloads; it is available for
/// schema-addressed storage, migration, and debugging use cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// Schema fingerprint / id identifying the payload's contract.
    pub schema_id: Vec<u8>,
    /// Schema version string (e.g. `"user.v1"`).
    pub schema_version: String,
    /// The opaque encoded payload bytes.
    pub payload: Vec<u8>,
}

impl Envelope {
    /// Encodes the envelope as a length-delimited blob using the native format.
    ///
    /// Layout (field ids are part of the envelope's own contract):
    /// - field 1: `schema_id` (bytes)
    /// - field 2: `schema_version` (string)
    /// - field 3: `payload` (bytes)
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Len,
            Value::Len(self.schema_id.clone()),
        ));
        m.push(Field::new(
            2,
            WireClass::Len,
            Value::Len(self.schema_version.as_bytes().to_vec()),
        ));
        m.push(Field::new(
            3,
            WireClass::Len,
            Value::Len(self.payload.clone()),
        ));
        m.encode()
    }

    /// Decodes an envelope from its native-encoded bytes.
    pub fn decode(bytes: &[u8], limits: &DecoderLimits) -> Result<Envelope, DecodeError> {
        let m = RawMessage::decode(bytes, limits, UnknownFieldPolicy::Preserve)?;
        let get = |id: u32| -> Result<Vec<u8>, DecodeError> {
            match m.fields.iter().find(|f| f.field_id == id) {
                Some(Field {
                    value: Value::Len(b),
                    ..
                }) => Ok(b.clone()),
                _ => Ok(Vec::new()),
            }
        };
        let schema_id = get(1)?;
        let schema_version = match m.fields.iter().find(|f| f.field_id == 2) {
            Some(Field {
                value: Value::Len(b),
                ..
            }) => String::from_utf8(b.clone()).map_err(|_| DecodeError::InvalidUtf8)?,
            _ => String::new(),
        };
        let payload = get(3)?;
        Ok(Envelope {
            schema_id,
            schema_version,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let env = Envelope {
            schema_id: b"fp".to_vec(),
            schema_version: "user.v1".to_string(),
            payload: b"data".to_vec(),
        };
        let bytes = env.encode().unwrap();
        let back = Envelope::decode(&bytes, &DecoderLimits::default()).unwrap();
        assert_eq!(env, back);
    }
}

//! Low-level message value model, encoder, and decoder for the native wire
//! format (spec §9, §11).
//!
//! This module provides the building blocks used both by generated code and by
//! the dynamic [`crate::dynamic`] message layer. It operates on a neutral
//! field model so unknown fields can be preserved and re-emitted.

use crate::error::{DecodeError, EncodeError};
use crate::limits::{DecoderLimits, UnknownFieldPolicy};
use crate::varint::{decode_varint, encode_varint, encode_varint_vec};
use crate::wire::{Tag, WireClass};

/// A decoded scalar value in the native wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A varint payload (covers bool, int/uint/sint, enums, sub-field length).
    Varint(u64),
    /// A 32-bit fixed-width little-endian value.
    Fixed32(u32),
    /// A 64-bit fixed-width little-endian value.
    Fixed64(u64),
    /// A length-delimited payload (string, bytes, packed repeated, nested
    /// message, map entry, or unknown length-delimited blob).
    Len(Vec<u8>),
}

/// A single field occurrence on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field identifier.
    pub field_id: u32,
    /// The wire class determined by the tag.
    pub wire_class: WireClass,
    /// The decoded value.
    pub value: Value,
}

impl Field {
    /// Constructs a field from its parts.
    pub fn new(field_id: u32, wire_class: WireClass, value: Value) -> Field {
        Field {
            field_id,
            wire_class,
            value,
        }
    }
}

/// A decoded message as an ordered list of fields. Unknown fields are retained
/// so the message can be re-encoded losslessly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawMessage {
    /// Fields in encounter order (callers may sort for canonical encoding).
    pub fields: Vec<Field>,
}

impl RawMessage {
    /// Creates an empty message.
    pub fn new() -> RawMessage {
        RawMessage::default()
    }

    /// Pushes a field onto the message.
    pub fn push(&mut self, field: Field) {
        self.fields.push(field);
    }

    /// Appends the fields of `other` to this message (used when folding
    /// duplicate map entries / oneof later-wins behavior at higher layers).
    pub fn extend(&mut self, other: RawMessage) {
        self.fields.extend(other.fields);
    }

    /// Decodes a message from `bytes` using the supplied limits.
    ///
    /// Every field is treated as schema-known, so this is a structural decode:
    /// the `_policy` parameter is accepted for signature compatibility but
    /// behaves as [`UnknownFieldPolicy::Preserve`] over all fields. Schema-aware
    /// callers (generated code, reflection) should use
    /// [`RawMessage::decode_filtered`].
    pub fn decode(
        bytes: &[u8],
        limits: &DecoderLimits,
        _policy: UnknownFieldPolicy,
    ) -> Result<RawMessage, DecodeError> {
        RawMessage::decode_filtered(bytes, limits, UnknownFieldPolicy::Preserve, &|_| true)
    }

    /// Decodes a message applying [`UnknownFieldPolicy`] relative to a
    /// schema-known predicate (spec §9.9):
    ///
    /// - `Preserve`: unknown fields are retained and remain re-encodable; the
    ///   total preserved size is bounded by `max_unknown_field_bytes`;
    /// - `Discard`: unknown fields are silently dropped;
    /// - `Fail`: the first unknown field aborts decoding with
    ///   [`DecodeError::UnknownFieldForbidden`].
    ///
    /// All structural limits (`max_message_bytes`, `max_field_count`,
    /// per-payload byte limits) are enforced regardless of policy.
    pub fn decode_filtered(
        bytes: &[u8],
        limits: &DecoderLimits,
        policy: UnknownFieldPolicy,
        is_known: &dyn Fn(u32) -> bool,
    ) -> Result<RawMessage, DecodeError> {
        limits.check_message_bytes(bytes.len())?;
        let mut msg = RawMessage::new();
        let mut cursor = 0usize;
        let mut field_count = 0usize;
        let mut unknown_bytes = 0usize;
        while cursor < bytes.len() {
            if field_count >= limits.max_field_count {
                return Err(DecodeError::FieldCountExceeded);
            }
            let tag_value = decode_varint(bytes, &mut cursor)?;
            let tag = Tag::from_u64(tag_value)?;
            let known = is_known(tag.field_id);
            if !known && policy == UnknownFieldPolicy::Fail {
                return Err(DecodeError::UnknownFieldForbidden);
            }
            let value = match tag.wire_class {
                WireClass::Varint => Value::Varint(decode_varint(bytes, &mut cursor)?),
                WireClass::Fixed32 => {
                    let b = read_fixed(bytes, &mut cursor, 4)?;
                    Value::Fixed32(u32::from_le_bytes(b[..4].try_into().expect("4 bytes")))
                }
                WireClass::Fixed64 => {
                    let b = read_fixed(bytes, &mut cursor, 8)?;
                    Value::Fixed64(u64::from_le_bytes(b))
                }
                WireClass::Len => {
                    // Zero-copy slice then copy once into storage: allocation is
                    // bounded by the validated length (spec §18.5).
                    let payload = split_len_delimited(bytes, &mut cursor, limits)?;
                    Value::Len(payload.to_vec())
                }
            };
            field_count += 1;
            if known {
                msg.push(Field::new(tag.field_id, tag.wire_class, value));
            } else if policy == UnknownFieldPolicy::Preserve {
                unknown_bytes += encoded_field_size(tag.field_id, tag.wire_class, &value);
                if unknown_bytes > limits.max_unknown_field_bytes {
                    return Err(DecodeError::LimitExceeded {
                        limit: limits.max_unknown_field_bytes,
                    });
                }
                msg.push(Field::new(tag.field_id, tag.wire_class, value));
            }
            // Discard: nothing to retain for unknown fields.
        }
        Ok(msg)
    }

    /// Encodes the message to a freshly allocated buffer.
    ///
    /// Field order is preserved as encountered; for canonical/deterministic
    /// output call [`RawMessage::encode_canonical`] after sorting. Length and
    /// varint payloads are emitted in their natural (minimal) form. Unknown
    /// fields are re-emitted as received.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        encode_message(self, &mut out)?;
        Ok(out)
    }

    /// Encodes the message in canonical/deterministic form.
    ///
    /// Fields are ordered by `(field_id, wire_class, payload)` — this total
    /// order also fixes the relative ordering of unknown fields (spec §9.10
    /// "unknown field ordering"). Length and varint payloads are emitted in
    /// their natural (minimal) form, so the output is suitable for hashing,
    /// signing, auditing, reproducible builds, and content addressing.
    ///
    /// Schema-aware canonical encoding should first call
    /// [`RawMessage::canonical_reduce_oneofs`] (oneof last-wins) and
    /// [`RawMessage::canonical_sort_map_entries`] (map ordering).
    pub fn encode_canonical(&self) -> Result<Vec<u8>, EncodeError> {
        let mut sorted = self.fields.clone();
        sorted.sort_by(|a, b| {
            a.field_id
                .cmp(&b.field_id)
                .then_with(|| (a.wire_class as u8).cmp(&(b.wire_class as u8)))
                .then_with(|| canonical_payload(a).cmp(&canonical_payload(b)))
        });
        let msg = RawMessage { fields: sorted };
        msg.encode()
    }

    /// Applies canonical oneof serialization behavior (spec §9.8, §9.10):
    /// within each group of mutually exclusive field ids, only the last
    /// occurrence on the wire survives ("last one wins").
    ///
    /// `groups` lists oneofs as slices of their member field ids.
    pub fn canonical_reduce_oneofs(&mut self, groups: &[&[u32]]) {
        for group in groups {
            let Some(keep_idx) = self.fields.iter().rposition(|f| group.contains(&f.field_id))
            else {
                continue;
            };
            let keep_id = self.fields[keep_idx].field_id;
            let mut i = 0usize;
            self.fields.retain(|f| {
                let drop = i < keep_idx && group.contains(&f.field_id);
                i += 1;
                !drop
            });
            // Exactly one member of the group remains, and it is the one that
            // appeared last on the wire.
            let survivors: Vec<u32> = self
                .fields
                .iter()
                .filter(|f| group.contains(&f.field_id))
                .map(|f| f.field_id)
                .collect();
            debug_assert_eq!(survivors, vec![keep_id]);
        }
    }

    /// Applies canonical map ordering (spec §9.7, §9.10): entries belonging to
    /// the given map field ids are stably sorted by their encoded key bytes,
    /// so duplicate-key folding order is deterministic regardless of input
    /// order.
    ///
    /// `map_field_ids` lists the synthetic repeated map-entry field ids.
    pub fn canonical_sort_map_entries(&mut self, map_field_ids: &[u32]) {
        let positions: Vec<usize> = self
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| map_field_ids.contains(&f.field_id) && matches!(f.value, Value::Len(_)))
            .map(|(i, _)| i)
            .collect();
        if positions.len() < 2 {
            return;
        }
        let mut entries: Vec<Field> = positions.iter().map(|&i| self.fields[i].clone()).collect();
        entries.sort_by(|a, b| {
            let ka = match &a.value {
                Value::Len(v) => map_entry_sort_key(v),
                _ => Vec::new(),
            };
            let kb = match &b.value {
                Value::Len(v) => map_entry_sort_key(v),
                _ => Vec::new(),
            };
            ka.cmp(&kb).then_with(|| {
                // Tie-break on the full entry so equal keys are deterministic.
                canonical_payload(a).cmp(&canonical_payload(b))
            })
        });
        for (slot, entry) in positions.into_iter().zip(entries) {
            self.fields[slot] = entry;
        }
    }
}

/// Extracts the encoded key bytes from a serialized map-entry message
/// (`field 1 = key`). Keys within one map share a type, so raw byte order is a
/// consistent deterministic order. Falls back to the whole entry on malformed
/// input (canonicalization never fails; it only orders).
fn map_entry_sort_key(entry: &[u8]) -> Vec<u8> {
    let mut cursor = 0usize;
    let Ok(tag_value) = decode_varint(entry, &mut cursor) else {
        return entry.to_vec();
    };
    let Ok(tag) = Tag::from_u64(tag_value) else {
        return entry.to_vec();
    };
    match tag.wire_class {
        WireClass::Varint => entry[cursor..].to_vec(),
        WireClass::Fixed32 | WireClass::Fixed64 => {
            let n = if tag.wire_class == WireClass::Fixed32 { 4 } else { 8 };
            if cursor + n <= entry.len() {
                entry[cursor..cursor + n].to_vec()
            } else {
                entry.to_vec()
            }
        }
        WireClass::Len => {
            let Ok(len) = decode_varint(entry, &mut cursor) else {
                return entry.to_vec();
            };
            let len = len as usize;
            if cursor.checked_add(len).is_some_and(|end| end <= entry.len()) {
                entry[cursor..cursor + len].to_vec()
            } else {
                entry.to_vec()
            }
        }
    }
}

fn canonical_payload(f: &Field) -> Vec<u8> {
    match &f.value {
        Value::Varint(v) => encode_varint_vec_canon(*v),
        Value::Fixed32(v) => v.to_le_bytes().to_vec(),
        Value::Fixed64(v) => v.to_le_bytes().to_vec(),
        Value::Len(v) => v.clone(),
    }
}

fn encode_varint_vec_canon(value: u64) -> Vec<u8> {
    encode_varint_vec(value)
}

/// Returns an unowned slice for the length-delimited payload starting at
/// `cursor`, advancing it past the payload (the tag must already be consumed).
/// Bounds- and limit-checked with no allocation; used by recursive
/// nested-message decoding so allocations stay proportional to validated
/// input (spec §18.5).
pub fn split_len_delimited<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    limits: &DecoderLimits,
) -> Result<&'a [u8], DecodeError> {
    let len = decode_varint(bytes, cursor)? as usize;
    let end = cursor.checked_add(len).ok_or(DecodeError::InvalidLength)?;
    if end > bytes.len() {
        return Err(DecodeError::Truncated);
    }
    if len > limits.max_bytes_field_bytes {
        return Err(DecodeError::LimitExceeded {
            limit: limits.max_bytes_field_bytes,
        });
    }
    let out = &bytes[*cursor..end];
    *cursor = end;
    Ok(out)
}

/// Size in bytes this field would occupy when re-encoded (tag + payload), used
/// to bound preserved-unknown memory growth.
fn encoded_field_size(field_id: u32, class: WireClass, value: &Value) -> usize {
    let tag_len = encode_varint_vec(Tag::new(field_id, class).to_u64()).len();
    let payload_len = match value {
        Value::Varint(v) => encode_varint_vec(*v).len(),
        Value::Fixed32(_) => 4,
        Value::Fixed64(_) => 8,
        Value::Len(v) => encode_varint_vec(v.len() as u64).len() + v.len(),
    };
    tag_len + payload_len
}

fn read_fixed(bytes: &[u8], cursor: &mut usize, n: usize) -> Result<[u8; 8], DecodeError> {
    if *cursor + n > bytes.len() {
        return Err(DecodeError::Truncated);
    }
    let mut buf = [0u8; 8];
    buf[..n].copy_from_slice(&bytes[*cursor..*cursor + n]);
    *cursor += n;
    Ok(buf)
}

fn encode_message(msg: &RawMessage, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    for field in &msg.fields {
        let tag = Tag::new(field.field_id, field.wire_class).to_u64();
        encode_varint(tag, out);
        match &field.value {
            Value::Varint(v) => encode_varint(*v, out),
            Value::Fixed32(v) => out.extend_from_slice(&v.to_le_bytes()),
            Value::Fixed64(v) => out.extend_from_slice(&v.to_le_bytes()),
            Value::Len(payload) => {
                encode_varint(payload.len() as u64, out);
                out.extend_from_slice(payload);
            }
        }
    }
    Ok(())
}

/// Re-export so callers can build canonical varints without reaching into the
/// private helper above.
pub use crate::varint::encode_varint_vec as canonical_varint;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::WireClass;

    #[test]
    fn roundtrip_simple() {
        let mut msg = RawMessage::new();
        msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
        msg.push(Field::new(2, WireClass::Len, Value::Len(b"hello".to_vec())));
        msg.push(Field::new(3, WireClass::Fixed64, Value::Fixed64(7)));
        let bytes = msg.encode().unwrap();
        let back = RawMessage::decode(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        )
        .unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn canonical_is_deterministic() {
        let mut a = RawMessage::new();
        a.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
        a.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
        let mut b = RawMessage::new();
        b.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
        b.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
        assert_eq!(a.encode_canonical().unwrap(), b.encode_canonical().unwrap());
    }

    #[test]
    fn rejects_truncated_fixed() {
        let bytes = encode_varint_vec(Tag::new(1, WireClass::Fixed64).to_u64());
        // Missing the 8 fixed bytes after the tag.
        let res = RawMessage::decode(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        );
        assert_eq!(res, Err(DecodeError::Truncated));
    }

    #[test]
    fn rejects_overlong_len() {
        let mut bytes = encode_varint_vec(Tag::new(1, WireClass::Len).to_u64());
        bytes.extend_from_slice(&encode_varint_vec(10));
        // Declared length 10 but no payload follows.
        let res = RawMessage::decode(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
        );
        assert_eq!(res, Err(DecodeError::Truncated));
    }

    fn field_bytes(field: &Field) -> Vec<u8> {
        RawMessage { fields: vec![field.clone()] }.encode().unwrap()
    }

    #[test]
    fn unknown_policy_discard_and_fail() {
        let mut msg = RawMessage::new();
        msg.push(Field::new(1, WireClass::Varint, Value::Varint(7)));
        msg.push(Field::new(99, WireClass::Len, Value::Len(b"zz".to_vec())));
        let bytes = msg.encode().unwrap();
        let known = |id: u32| id == 1;

        let kept = RawMessage::decode_filtered(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Preserve,
            &known,
        )
        .unwrap();
        assert_eq!(kept.fields.len(), 2);

        let dropped = RawMessage::decode_filtered(
            &bytes,
            &DecoderLimits::default(),
            UnknownFieldPolicy::Discard,
            &known,
        )
        .unwrap();
        assert_eq!(dropped.fields.len(), 1);
        assert_eq!(dropped.fields[0].field_id, 1);

        assert_eq!(
            RawMessage::decode_filtered(
                &bytes,
                &DecoderLimits::default(),
                UnknownFieldPolicy::Fail,
                &known
            ),
            Err(DecodeError::UnknownFieldForbidden)
        );
    }

    #[test]
    fn unknown_budget_is_enforced() {
        let f = Field::new(200, WireClass::Len, Value::Len(vec![0u8; 32]));
        let bytes = field_bytes(&f);
        let limits = DecoderLimits {
            max_unknown_field_bytes: 8,
            ..DecoderLimits::default()
        };
        assert_eq!(
            RawMessage::decode_filtered(&bytes, &limits, UnknownFieldPolicy::Preserve, &|_| false),
            Err(DecodeError::LimitExceeded { limit: 8 })
        );
        // The same payload under a known id is not charged to the unknown budget.
        let f2 = Field::new(1, WireClass::Len, Value::Len(vec![0u8; 32]));
        let bytes2 = field_bytes(&f2);
        assert!(RawMessage::decode_filtered(
            &bytes2,
            &limits,
            UnknownFieldPolicy::Preserve,
            &|id| id == 1
        )
        .is_ok());
    }

    #[test]
    fn message_size_limit_enforced() {
        let bytes = vec![0u8; 64];
        let limits = DecoderLimits {
            max_message_bytes: 16,
            ..DecoderLimits::default()
        };
        assert_eq!(
            RawMessage::decode(&bytes, &limits, UnknownFieldPolicy::Preserve),
            Err(DecodeError::LimitExceeded { limit: 16 })
        );
    }

    #[test]
    fn canonical_oneof_last_wins() {
        let mut msg = RawMessage::new();
        msg.push(Field::new(10, WireClass::Varint, Value::Varint(1)));
        msg.push(Field::new(11, WireClass::Varint, Value::Varint(2)));
        msg.push(Field::new(10, WireClass::Varint, Value::Varint(3)));
        msg.canonical_reduce_oneofs(&[&[10, 11]]);
        let ids: Vec<u32> = msg.fields.iter().map(|f| f.field_id).collect();
        assert_eq!(ids, vec![10]);
        assert_eq!(msg.fields[0].value, Value::Varint(3));
    }

    #[test]
    fn canonical_map_entry_ordering() {
        // Map field 5 with string keys: entries out of order on the wire.
        let entry = |k: &str, v: u64| -> Field {
            let mut e = RawMessage::new();
            e.push(Field::new(1, WireClass::Len, Value::Len(k.as_bytes().to_vec())));
            e.push(Field::new(2, WireClass::Varint, Value::Varint(v)));
            Field::new(5, WireClass::Len, Value::Len(e.encode().unwrap()))
        };
        let mut a = RawMessage::new();
        a.push(entry("zebra", 1));
        a.push(entry("apple", 2));
        a.push(Field::new(1, WireClass::Varint, Value::Varint(9)));
        let mut b = RawMessage::new();
        b.push(Field::new(1, WireClass::Varint, Value::Varint(9)));
        b.push(entry("apple", 2));
        b.push(entry("zebra", 1));

        a.canonical_sort_map_entries(&[5]);
        b.canonical_sort_map_entries(&[5]);
        assert_eq!(a.encode_canonical().unwrap(), b.encode_canonical().unwrap());
        // And the map entries are now key-ordered (ascending by encoded key).
        match &a.fields[0].value {
            Value::Len(e) => assert_eq!(map_entry_sort_key(e), b"apple".to_vec()),
            _ => panic!("expected len payload"),
        }
    }

    #[test]
    fn canonical_unknown_field_ordering_is_total() {
        let mk = |id: u32| -> Vec<u8> {
            let mut m = RawMessage::new();
            m.push(Field::new(id, WireClass::Varint, Value::Varint(1)));
            m.encode_canonical().unwrap()
        };
        let ab = [mk(3), mk(1), mk(2)].concat();
        let ba = [mk(2), mk(3), mk(1)].concat();
        let dec = |b: &[u8]| {
            RawMessage::decode(b, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap()
        };
        assert_eq!(
            dec(&ab).encode_canonical().unwrap(),
            dec(&ba).encode_canonical().unwrap()
        );
    }

    #[test]
    fn split_len_delimited_bounds() {
        let limits = DecoderLimits::default();
        let mut bytes = encode_varint_vec(4);
        bytes.extend_from_slice(b"abcd");
        let mut cursor = 0usize;
        let slice = split_len_delimited(&bytes, &mut cursor, &limits).unwrap();
        assert_eq!(slice, b"abcd");
        assert_eq!(cursor, bytes.len());
        // Truncated declared length.
        assert_eq!(
            split_len_delimited(&encode_varint_vec(9), &mut 0, &limits),
            Err(DecodeError::Truncated)
        );
        // Declared length beyond the per-field limit.
        let big = DecoderLimits {
            max_bytes_field_bytes: 2,
            ..DecoderLimits::default()
        };
        assert_eq!(
            split_len_delimited(&bytes, &mut 0, &big),
            Err(DecodeError::LimitExceeded { limit: 2 })
        );
    }
}

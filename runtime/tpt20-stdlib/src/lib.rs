//! `tpt20-stdlib`: standard library types for tpt20 (spec §15).
//!
//! This crate provides the well-known message types that ship with every
//! `tpt20` installation. Types are defined as plain Rust structs/enums so
//! they can be used directly, and the accompanying `.tpt` schemas (in
//! `src/schema/`) are the canonical source of truth for code generation and
//! descriptor exchange.
//!
//! ## Stability
//!
//! The standard library follows the stability policy in `STABILITY.md`.
//! Fields within these messages are part of the wire contract: adding new
//! optional fields is safe; removing or changing existing field IDs is
//! breaking.

pub mod json;
pub mod schema;

use std::collections::BTreeMap;

use tpt20_core::{DecodeError, Field, RawMessage, WireClass};

// ---- Core standard types ---------------------------------------------------

/// A point in time, represented as seconds since the Unix epoch plus
/// fractional nanoseconds (spec §15 `Timestamp`).
///
/// Wire layout:
/// - field 1: `seconds` (int64, varint)
/// - field 2: `nanos` (int32, varint)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp {
    /// Seconds since the Unix epoch (1970-01-01T00:00:00Z). Negative values
    /// represent times before the epoch.
    pub seconds: i64,
    /// Nanoseconds within the second, in the range `[0, 999_999_999]`.
    pub nanos: i32,
}

impl Default for Timestamp {
    fn default() -> Self {
        Timestamp {
            seconds: 0,
            nanos: 0,
        }
    }
}

/// A signed, fixed-length span of time, represented as seconds plus
/// nanoseconds (spec §15 `Duration`).
///
/// Wire layout:
/// - field 1: `seconds` (int64, varint)
/// - field 2: `nanos` (int32, varint)
///
/// `nanos` may be negative in the range `[-999_999_999, 0]` when `seconds` is
/// positive, or positive in the range `[0, 999_999_999]` when `seconds` is
/// negative, such that the overall sign is carried by `seconds`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duration {
    /// Signed seconds component.
    pub seconds: i64,
    /// Nanoseconds component; same sign as `seconds` or opposite with smaller
    /// magnitude.
    pub nanos: i32,
}

impl Default for Duration {
    fn default() -> Self {
        Duration {
            seconds: 0,
            nanos: 0,
        }
    }
}

/// An empty message that carries no data (spec §15 `Empty`).
///
/// Wire layout: no fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Empty;

/// A dynamically typed value that carries a type URL and opaque bytes
/// (spec §15 `Any`).
///
/// Wire layout:
/// - field 1: `type_url` (string)
/// - field 2: `value` (bytes)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Any {
    /// A URL that identifies the concrete type of the value.
    pub type_url: String,
    /// The serialized value bytes.
    pub value: Vec<u8>,
}

impl Default for Any {
    fn default() -> Self {
        Any {
            type_url: String::new(),
            value: Vec::new(),
        }
    }
}

/// A structured value equivalent to a JSON object (spec §15 `Struct`).
///
/// Wire layout:
/// - field 1: `fields` (map<string, StdValue>)
#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    /// Unordered map of field name to value.
    pub fields: BTreeMap<String, StdValue>,
}

impl Default for Struct {
    fn default() -> Self {
        Struct {
            fields: BTreeMap::new(),
        }
    }
}

/// A dynamically typed value, mirroring a JSON value (spec §15 `Value`).
///
/// Wire layout:
/// - field 1: `null_value` (NullValue, enum)
/// - field 2: `number_value` (double, fixed64)
/// - field 3: `string_value` (string)
/// - field 4: `bool_value` (bool)
/// - field 5: `struct_value` (Struct)
/// - field 6: `list_value` (ListValue)
///
/// Only one field should be set; this is not enforced at the type level but
/// is the convention.
#[derive(Debug, Clone, PartialEq)]
pub enum StdValue {
    Null(NullValue),
    Number(f64),
    String(String),
    Bool(bool),
    Struct(Struct),
    List(ListValue),
}

/// The empty JSON null value (spec §15 `NullValue`).
///
/// Wire layout: enum with a single value `NULL_VALUE = 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullValue {
    /// The null value.
    NullValue = 0,
}

impl Default for NullValue {
    fn default() -> Self {
        NullValue::NullValue
    }
}

/// A list of `StdValue` objects (spec §15 `ListValue`).
///
/// Wire layout:
/// - field 1: `values` (repeated StdValue)
#[derive(Debug, Clone, PartialEq)]
pub struct ListValue {
    /// Ordered list of values.
    pub values: Vec<StdValue>,
}

impl Default for ListValue {
    fn default() -> Self {
        ListValue {
            values: Vec::new(),
        }
    }
}

/// A set of field paths used to select a subset of fields on an object
/// (spec §15 `FieldMask`).
///
/// Wire layout:
/// - field 1: `paths` (repeated string)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMask {
    /// Dot-separated paths identifying fields to retain.
    pub paths: Vec<String>,
}

impl Default for FieldMask {
    fn default() -> Self {
        FieldMask {
            paths: Vec::new(),
        }
    }
}

/// A UUID, represented as a string (spec §15 `UUID`).
///
/// Wire layout:
/// - field 1: `value` (string)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uuid {
    /// The UUID string (e.g. `"550e8400-e29b-41d4-a716-446655440000"`).
    pub value: String,
}

impl Default for Uuid {
    fn default() -> Self {
        Uuid {
            value: String::new(),
        }
    }
}

/// An arbitrary-precision decimal value (spec §15 `Decimal`).
///
/// Wire layout:
/// - field 1: `value` (bytes)
///
/// The `value` bytes encode the decimal in an implementation-defined format.
/// Consumers should agree on a serialization convention (e.g. BCD, string
/// UTF-8, or a binary significand/exponent encoding) before exchanging
/// `Decimal` messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimal {
    /// Opaque decimal encoding.
    pub value: Vec<u8>,
}

impl Default for Decimal {
    fn default() -> Self {
        Decimal {
            value: Vec::new(),
        }
    }
}

/// A monetary value with currency code (spec §15 `Money`).
///
/// Wire layout:
/// - field 1: `currency_code` (string, 3-character ISO 4217)
/// - field 2: `units` (int64, whole units)
/// - field 3: `nanos` (int32, fractional units, 0 to 999,999,999)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money {
    /// Three-character ISO 4217 currency code (e.g. `"USD"`).
    pub currency_code: String,
    /// Whole units of the currency (e.g. 1 for $1.00).
    pub units: i64,
    /// Nanoseconds of the currency, in the range `[0, 999_999_999]`.
    pub nanos: i32,
}

impl Default for Money {
    fn default() -> Self {
        Money {
            currency_code: String::new(),
            units: 0,
            nanos: 0,
        }
    }
}

/// A closed time interval, identified by its start and end points
/// (spec §15 `Interval`).
///
/// Wire layout:
/// - field 1: `start` (Timestamp)
/// - field 2: `end` (Timestamp)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    /// Inclusive start of the interval.
    pub start: Timestamp,
    /// Exclusive end of the interval.
    pub end: Timestamp,
}

impl Default for Interval {
    fn default() -> Self {
        Interval {
            start: Timestamp::default(),
            end: Timestamp::default(),
        }
    }
}

/// Pagination state returned by list operations (spec §15 `Pagination`).
///
/// Wire layout:
/// - field 1: `page_token` (string)
/// - field 2: `page_size` (int32, optional)
/// - field 3: `total` (int64, optional)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pagination {
    /// Opaque token identifying the next page; empty when no more pages exist.
    pub page_token: String,
    /// Requested or actual page size.
    pub page_size: Option<i32>,
    /// Total number of items across all pages, when known.
    pub total: Option<i64>,
}

impl Default for Pagination {
    fn default() -> Self {
        Pagination {
            page_token: String::new(),
            page_size: None,
            total: None,
        }
    }
}

/// Structured error detail returned alongside an RPC status
/// (spec §15 `ErrorDetail`).
///
/// Wire layout:
/// - field 1: `code` (string)
/// - field 2: `message` (string)
/// - field 3: `details` (repeated Any)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDetail {
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable description.
    pub message: String,
    /// Structured sub-details.
    pub details: Vec<Any>,
}

impl Default for ErrorDetail {
    fn default() -> Self {
        ErrorDetail {
            code: String::new(),
            message: String::new(),
            details: Vec::new(),
        }
    }
}

// ---- Wrapper types ---------------------------------------------------------

/// Wrapper for `bool` using explicit presence (spec §15 `BoolValue`).
///
/// Wire layout:
/// - field 1: `value` (bool, optional)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoolValue {
    /// Wrapped boolean; `None` means "unset".
    pub value: Option<bool>,
}

impl Default for BoolValue {
    fn default() -> Self {
        BoolValue { value: None }
    }
}

/// Wrapper for `bytes` using explicit presence (spec §15 `BytesValue`).
///
/// Wire layout:
/// - field 1: `value` (bytes, optional)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytesValue {
    /// Wrapped bytes; `None` means "unset".
    pub value: Option<Vec<u8>>,
}

impl Default for BytesValue {
    fn default() -> Self {
        BytesValue { value: None }
    }
}

/// Wrapper for `double` using explicit presence (spec §15 `DoubleValue`).
///
/// Wire layout:
/// - field 1: `value` (double, optional)
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleValue {
    /// Wrapped 64-bit float; `None` means "unset".
    pub value: Option<f64>,
}

impl Default for DoubleValue {
    fn default() -> Self {
        DoubleValue { value: None }
    }
}

/// Wrapper for `float32` using explicit presence (spec §15 `FloatValue`).
///
/// Wire layout:
/// - field 1: `value` (float32, optional)
#[derive(Debug, Clone, PartialEq)]
pub struct FloatValue {
    /// Wrapped 32-bit float; `None` means "unset".
    pub value: Option<f32>,
}

impl Default for FloatValue {
    fn default() -> Self {
        FloatValue { value: None }
    }
}

/// Wrapper for `int32` using explicit presence (spec §15 `Int32Value`).
///
/// Wire layout:
/// - field 1: `value` (int32, optional)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Int32Value {
    /// Wrapped 32-bit signed integer; `None` means "unset".
    pub value: Option<i32>,
}

impl Default for Int32Value {
    fn default() -> Self {
        Int32Value { value: None }
    }
}

/// Wrapper for `int64` using explicit presence (spec §15 `Int64Value`).
///
/// Wire layout:
/// - field 1: `value` (int64, optional)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Int64Value {
    /// Wrapped 64-bit signed integer; `None` means "unset".
    pub value: Option<i64>,
}

impl Default for Int64Value {
    fn default() -> Self {
        Int64Value { value: None }
    }
}

/// Wrapper for `uint32` using explicit presence (spec §15 `UInt32Value`).
///
/// Wire layout:
/// - field 1: `value` (uint32, optional)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UInt32Value {
    /// Wrapped 32-bit unsigned integer; `None` means "unset".
    pub value: Option<u32>,
}

impl Default for UInt32Value {
    fn default() -> Self {
        UInt32Value { value: None }
    }
}

/// Wrapper for `uint64` using explicit presence (spec §15 `UInt64Value`).
///
/// Wire layout:
/// - field 1: `value` (uint64, optional)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UInt64Value {
    /// Wrapped 64-bit unsigned integer; `None` means "unset".
    pub value: Option<u64>,
}

impl Default for UInt64Value {
    fn default() -> Self {
        UInt64Value { value: None }
    }
}

/// Wrapper for `string` using explicit presence (spec §15 `StringValue`).
///
/// Wire layout:
/// - field 1: `value` (string, optional)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringValue {
    /// Wrapped string; `None` means "unset".
    pub value: Option<String>,
}

impl Default for StringValue {
    fn default() -> Self {
        StringValue { value: None }
    }
}

// ---- Raw message encode/decode helpers -------------------------------------

impl Timestamp {
    /// Creates a new `Timestamp`.
    pub fn new(seconds: i64, nanos: i32) -> Self {
        Timestamp { seconds, nanos }
    }

    /// Encodes the timestamp as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Varint,
            tpt20_core::Value::Varint(self.seconds as u64),
        ));
        m.push(Field::new(
            2,
            WireClass::Varint,
            tpt20_core::Value::Varint(self.nanos as u64),
        ));
        m.encode()
    }

    /// Decodes a timestamp from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Timestamp, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut ts = Timestamp::default();
        for f in &raw.fields {
            match f.field_id {
                1 => ts.seconds = tpt20_core::scalar::decode_signed(&f.value)?,
                2 => ts.nanos = tpt20_core::scalar::decode_signed(&f.value)? as i32,
                _ => {}
            }
        }
        Ok(ts)
    }
}

impl Duration {
    /// Creates a new `Duration`.
    pub fn new(seconds: i64, nanos: i32) -> Self {
        Duration { seconds, nanos }
    }

    /// Encodes the duration as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Varint,
            tpt20_core::Value::Varint(self.seconds as u64),
        ));
        m.push(Field::new(
            2,
            WireClass::Varint,
            tpt20_core::Value::Varint(self.nanos as u64),
        ));
        m.encode()
    }

    /// Decodes a duration from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Duration, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut d = Duration::default();
        for f in &raw.fields {
            match f.field_id {
                1 => d.seconds = tpt20_core::scalar::decode_signed(&f.value)?,
                2 => d.nanos = tpt20_core::scalar::decode_signed(&f.value)? as i32,
                _ => {}
            }
        }
        Ok(d)
    }
}

impl Any {
    /// Creates a new `Any`.
    pub fn new(type_url: String, value: Vec<u8>) -> Self {
        Any { type_url, value }
    }

    /// Encodes the `Any` as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Len,
            tpt20_core::Value::Len(self.type_url.as_bytes().to_vec()),
        ));
        m.push(Field::new(
            2,
            WireClass::Len,
            tpt20_core::Value::Len(self.value.clone()),
        ));
        m.encode()
    }

    /// Decodes an `Any` from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Any, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut a = Any::default();
        for f in &raw.fields {
            match f.field_id {
                1 => {
                    let bytes = tpt20_core::scalar::decode_bytes(&f.value)?;
                    a.type_url = std::str::from_utf8(bytes)
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string();
                }
                2 => a.value = tpt20_core::scalar::decode_bytes(&f.value)?.to_vec(),
                _ => {}
            }
        }
        Ok(a)
    }
}

impl Empty {
    /// Encodes the empty message (always an empty byte slice).
    pub fn encode() -> Result<Vec<u8>, tpt20_core::EncodeError> {
        Ok(Vec::new())
    }

    /// Decodes the empty message.
    pub fn decode(bytes: &[u8]) -> Result<Empty, DecodeError> {
        if bytes.is_empty() {
            Ok(Empty)
        } else {
            Err(DecodeError::Internal("Empty message must have no fields"))
        }
    }
}

impl Struct {
    /// Creates a new `Struct` from a field map.
    pub fn new(fields: BTreeMap<String, StdValue>) -> Self {
        Struct { fields }
    }

    /// Encodes the struct as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        for (name, value) in &self.fields {
            let mut entry = RawMessage::new();
            entry.push(Field::new(
                1,
                WireClass::Len,
                tpt20_core::Value::Len(name.as_bytes().to_vec()),
            ));
            entry.push(Field::new(2, WireClass::Len, tpt20_core::Value::Len(value.encode()?)));
            m.push(Field::new(1, WireClass::Len, tpt20_core::Value::Len(entry.encode()?)));
        }
        m.encode()
    }

    /// Decodes a struct from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Struct, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut s = Struct::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                let entry_bytes = tpt20_core::scalar::decode_bytes(&f.value)?;
                let entry = RawMessage::decode(
                    entry_bytes,
                    &tpt20_core::DecoderLimits::default(),
                    tpt20_core::UnknownFieldPolicy::Preserve,
                )?;
                let mut name = String::new();
                let mut val: Option<StdValue> = None;
                for ef in &entry.fields {
                    match ef.field_id {
                        1 => {
                            let b = tpt20_core::scalar::decode_bytes(&ef.value)?;
                            name = std::str::from_utf8(b)
                                .map_err(|_| DecodeError::InvalidUtf8)?
                                .to_string();
                        }
                        2 => val = Some(StdValue::decode_value(ef.value.clone())?),
                        _ => {}
                    }
                }
                if let Some(v) = val {
                    s.fields.insert(name, v);
                }
            }
        }
        Ok(s)
    }
}

impl StdValue {
    /// Encodes the value as length-delimited bytes (used as a field payload).
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        match self {
            StdValue::Null(_) => {
                m.push(Field::new(
                    1,
                    WireClass::Varint,
                    tpt20_core::Value::Varint(0),
                ));
            }
            StdValue::Number(v) => {
                m.push(Field::new(
                    2,
                    WireClass::Fixed64,
                    tpt20_core::Value::Fixed64(v.to_bits()),
                ));
            }
            StdValue::String(s) => {
                m.push(Field::new(
                    3,
                    WireClass::Len,
                    tpt20_core::Value::Len(s.as_bytes().to_vec()),
                ));
            }
            StdValue::Bool(b) => {
                m.push(Field::new(
                    4,
                    WireClass::Varint,
                    tpt20_core::Value::Varint(if *b { 1 } else { 0 }),
                ));
            }
            StdValue::Struct(s) => {
                m.push(Field::new(5, WireClass::Len, tpt20_core::Value::Len(s.encode()?)));
            }
            StdValue::List(l) => {
                m.push(Field::new(6, WireClass::Len, tpt20_core::Value::Len(l.encode()?)));
            }
        }
        m.encode()
    }

    /// Decodes a `StdValue` from its length-delimited payload.
    pub fn decode(bytes: &[u8]) -> Result<StdValue, DecodeError> {
        Self::decode_value(tpt20_core::Value::Len(bytes.to_vec()))
    }

    /// Decodes from a wire `tpt20_core::Value`.
    pub(crate) fn decode_value(value: tpt20_core::Value) -> Result<StdValue, DecodeError> {
        let payload = tpt20_core::scalar::decode_bytes(&value)?;
        let raw = RawMessage::decode(
            payload,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut v = None;
        for f in &raw.fields {
            match f.field_id {
                1 => v = Some(StdValue::Null(NullValue::NullValue)),
                2 => {
                    let bits = tpt20_core::scalar::decode_fixed64(&f.value)?;
                    v = Some(StdValue::Number(f64::from_bits(bits)));
                }
                3 => {
                    let b = tpt20_core::scalar::decode_bytes(&f.value)?;
                    v = Some(StdValue::String(
                        std::str::from_utf8(b)
                            .map_err(|_| DecodeError::InvalidUtf8)?
                            .to_string(),
                    ));
                }
                4 => {
                    let n = tpt20_core::scalar::decode_uint(&f.value)?;
                    v = Some(StdValue::Bool(n != 0));
                }
                5 => v = Some(StdValue::Struct(Struct::decode(
                    tpt20_core::scalar::decode_bytes(&f.value)?,
                )?)),
                6 => v = Some(StdValue::List(ListValue::decode(
                    tpt20_core::scalar::decode_bytes(&f.value)?,
                )?)),
                _ => {}
            }
        }
        v.ok_or_else(|| DecodeError::Internal("empty StdValue"))
    }
}

impl ListValue {
    /// Creates a new `ListValue`.
    pub fn new(values: Vec<StdValue>) -> Self {
        ListValue { values }
    }

    /// Encodes the list value as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        for v in &self.values {
            m.push(Field::new(1, WireClass::Len, tpt20_core::Value::Len(v.encode()?)));
        }
        m.encode()
    }

    /// Decodes a list value from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<ListValue, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut lv = ListValue::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                lv.values.push(StdValue::decode_value(f.value.clone())?);
            }
        }
        Ok(lv)
    }
}

impl FieldMask {
    /// Creates a new `FieldMask`.
    pub fn new(paths: Vec<String>) -> Self {
        FieldMask { paths }
    }

    /// Encodes the field mask as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        for p in &self.paths {
            m.push(Field::new(
                1,
                WireClass::Len,
                tpt20_core::Value::Len(p.as_bytes().to_vec()),
            ));
        }
        m.encode()
    }

    /// Decodes a field mask from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<FieldMask, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut fm = FieldMask::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                let b = tpt20_core::scalar::decode_bytes(&f.value)?;
                fm.paths.push(
                    std::str::from_utf8(b)
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string(),
                );
            }
        }
        Ok(fm)
    }
}

impl Uuid {
    /// Creates a new `Uuid`.
    pub fn new(value: String) -> Self {
        Uuid { value }
    }

    /// Encodes the UUID as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Len,
            tpt20_core::Value::Len(self.value.as_bytes().to_vec()),
        ));
        m.encode()
    }

    /// Decodes a UUID from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Uuid, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut u = Uuid::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                let b = tpt20_core::scalar::decode_bytes(&f.value)?;
                u.value = std::str::from_utf8(b)
                    .map_err(|_| DecodeError::InvalidUtf8)?
                    .to_string();
            }
        }
        Ok(u)
    }
}

impl Decimal {
    /// Creates a new `Decimal`.
    pub fn new(value: Vec<u8>) -> Self {
        Decimal { value }
    }

    /// Encodes the decimal as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Len,
            tpt20_core::Value::Len(self.value.clone()),
        ));
        m.encode()
    }

    /// Decodes a decimal from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Decimal, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut d = Decimal::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                d.value = tpt20_core::scalar::decode_bytes(&f.value)?.to_vec();
            }
        }
        Ok(d)
    }
}

impl Money {
    /// Creates a new `Money`.
    pub fn new(currency_code: String, units: i64, nanos: i32) -> Self {
        Money {
            currency_code,
            units,
            nanos,
        }
    }

    /// Encodes the money value as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Len,
            tpt20_core::Value::Len(self.currency_code.as_bytes().to_vec()),
        ));
        m.push(Field::new(
            2,
            WireClass::Varint,
            tpt20_core::Value::Varint(self.units as u64),
        ));
        m.push(Field::new(
            3,
            WireClass::Varint,
            tpt20_core::Value::Varint(self.nanos as u64),
        ));
        m.encode()
    }

    /// Decodes a money value from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Money, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut m = Money::default();
        for f in &raw.fields {
            match f.field_id {
                1 => {
                    let b = tpt20_core::scalar::decode_bytes(&f.value)?;
                    m.currency_code = std::str::from_utf8(b)
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string();
                }
                2 => m.units = tpt20_core::scalar::decode_signed(&f.value)?,
                3 => m.nanos = tpt20_core::scalar::decode_signed(&f.value)? as i32,
                _ => {}
            }
        }
        Ok(m)
    }
}

impl Interval {
    /// Creates a new `Interval`.
    pub fn new(start: Timestamp, end: Timestamp) -> Self {
        Interval { start, end }
    }

    /// Encodes the interval as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(1, WireClass::Len, tpt20_core::Value::Len(self.start.encode()?)));
        m.push(Field::new(2, WireClass::Len, tpt20_core::Value::Len(self.end.encode()?)));
        m.encode()
    }

    /// Decodes an interval from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Interval, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut i = Interval::default();
        for f in &raw.fields {
            match f.field_id {
                1 => {
                    i.start = Timestamp::decode(tpt20_core::scalar::decode_bytes(&f.value)?)?
                }
                2 => i.end = Timestamp::decode(tpt20_core::scalar::decode_bytes(&f.value)?)?,
                _ => {}
            }
        }
        Ok(i)
    }
}

impl Pagination {
    /// Creates a new `Pagination`.
    pub fn new(page_token: String, page_size: Option<i32>, total: Option<i64>) -> Self {
        Pagination {
            page_token,
            page_size,
            total,
        }
    }

    /// Encodes the pagination state as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Len,
            tpt20_core::Value::Len(self.page_token.as_bytes().to_vec()),
        ));
        if let Some(v) = self.page_size {
            m.push(Field::new(
                2,
                WireClass::Varint,
                tpt20_core::Value::Varint(v as u64),
            ));
        }
        if let Some(v) = self.total {
            m.push(Field::new(
                3,
                WireClass::Varint,
                tpt20_core::Value::Varint(v as u64),
            ));
        }
        m.encode()
    }

    /// Decodes pagination state from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Pagination, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut p = Pagination::default();
        for f in &raw.fields {
            match f.field_id {
                1 => {
                    let b = tpt20_core::scalar::decode_bytes(&f.value)?;
                    p.page_token = std::str::from_utf8(b)
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string();
                }
                2 => p.page_size = Some(tpt20_core::scalar::decode_uint(&f.value)? as i32),
                3 => p.total = Some(tpt20_core::scalar::decode_uint(&f.value)? as i64),
                _ => {}
            }
        }
        Ok(p)
    }
}

impl ErrorDetail {
    /// Creates a new `ErrorDetail`.
    pub fn new(code: String, message: String, details: Vec<Any>) -> Self {
        ErrorDetail {
            code,
            message,
            details,
        }
    }

    /// Encodes the error detail as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        m.push(Field::new(
            1,
            WireClass::Len,
            tpt20_core::Value::Len(self.code.as_bytes().to_vec()),
        ));
        m.push(Field::new(
            2,
            WireClass::Len,
            tpt20_core::Value::Len(self.message.as_bytes().to_vec()),
        ));
        for d in &self.details {
            m.push(Field::new(3, WireClass::Len, tpt20_core::Value::Len(d.encode()?)));
        }
        m.encode()
    }

    /// Decodes error detail from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<ErrorDetail, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut e = ErrorDetail::default();
        for f in &raw.fields {
            match f.field_id {
                1 => {
                    let b = tpt20_core::scalar::decode_bytes(&f.value)?;
                    e.code = std::str::from_utf8(b)
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string();
                }
                2 => {
                    let b = tpt20_core::scalar::decode_bytes(&f.value)?;
                    e.message = std::str::from_utf8(b)
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string();
                }
                3 => {
                    e.details.push(Any::decode(tpt20_core::scalar::decode_bytes(
                        &f.value,
                    )?)?);
                }
                _ => {}
            }
        }
        Ok(e)
    }
}

// ---- Wrapper type encode/decode --------------------------------------------

impl BoolValue {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Varint,
                tpt20_core::Value::Varint(if v { 1 } else { 0 }),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<BoolValue, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = BoolValue::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                let n = tpt20_core::scalar::decode_uint(&f.value)?;
                w.value = Some(n != 0);
            }
        }
        Ok(w)
    }
}

impl BytesValue {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(ref v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Len,
                tpt20_core::Value::Len(v.clone()),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<BytesValue, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = BytesValue::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                w.value = Some(tpt20_core::scalar::decode_bytes(&f.value)?.to_vec());
            }
        }
        Ok(w)
    }
}

impl DoubleValue {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Fixed64,
                tpt20_core::Value::Fixed64(v.to_bits()),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<DoubleValue, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = DoubleValue::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                let bits = tpt20_core::scalar::decode_fixed64(&f.value)?;
                w.value = Some(f64::from_bits(bits));
            }
        }
        Ok(w)
    }
}

impl FloatValue {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Fixed32,
                tpt20_core::Value::Fixed32(v.to_bits()),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<FloatValue, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = FloatValue::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                let bits = tpt20_core::scalar::decode_fixed32(&f.value)?;
                w.value = Some(f32::from_bits(bits));
            }
        }
        Ok(w)
    }
}

impl Int32Value {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Varint,
                tpt20_core::Value::Varint(v as u64),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Int32Value, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = Int32Value::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                w.value = Some(tpt20_core::scalar::decode_signed(&f.value)? as i32);
            }
        }
        Ok(w)
    }
}

impl Int64Value {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Varint,
                tpt20_core::Value::Varint(v as u64),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<Int64Value, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = Int64Value::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                w.value = Some(tpt20_core::scalar::decode_signed(&f.value)?);
            }
        }
        Ok(w)
    }
}

impl UInt32Value {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Varint,
                tpt20_core::Value::Varint(v as u64),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<UInt32Value, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = UInt32Value::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                w.value = Some(tpt20_core::scalar::decode_uint(&f.value)? as u32);
            }
        }
        Ok(w)
    }
}

impl UInt64Value {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Varint,
                tpt20_core::Value::Varint(v),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<UInt64Value, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = UInt64Value::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                w.value = Some(tpt20_core::scalar::decode_uint(&f.value)?);
            }
        }
        Ok(w)
    }
}

impl StringValue {
    /// Encodes the wrapper as a native tpt20 binary message.
    pub fn encode(&self) -> Result<Vec<u8>, tpt20_core::EncodeError> {
        let mut m = RawMessage::new();
        if let Some(ref v) = self.value {
            m.push(Field::new(
                1,
                WireClass::Len,
                tpt20_core::Value::Len(v.as_bytes().to_vec()),
            ));
        }
        m.encode()
    }

    /// Decodes the wrapper from native tpt20 binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<StringValue, DecodeError> {
        let raw = RawMessage::decode(
            bytes,
            &tpt20_core::DecoderLimits::default(),
            tpt20_core::UnknownFieldPolicy::Preserve,
        )?;
        let mut w = StringValue::default();
        for f in &raw.fields {
            if f.field_id == 1 {
                let b = tpt20_core::scalar::decode_bytes(&f.value)?;
                w.value = Some(
                    std::str::from_utf8(b)
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string(),
                );
            }
        }
        Ok(w)
    }
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_roundtrip() {
        let ts = Timestamp::new(1_234_567_890, 123_456_789);
        let bytes = ts.encode().unwrap();
        let back = Timestamp::decode(&bytes).unwrap();
        assert_eq!(ts, back);
    }

    #[test]
    fn duration_roundtrip() {
        let d = Duration::new(-42, -500_000_000);
        let bytes = d.encode().unwrap();
        let back = Duration::decode(&bytes).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn empty_roundtrip() {
        let bytes = Empty::encode().unwrap();
        assert!(bytes.is_empty());
        assert_eq!(Empty::decode(&bytes).unwrap(), Empty);
    }

    #[test]
    fn any_roundtrip() {
        let a = Any::new("type.googleapis.com/user.v1.User".into(), b"data".to_vec());
        let bytes = a.encode().unwrap();
        let back = Any::decode(&bytes).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn struct_roundtrip() {
        let mut fields = BTreeMap::new();
        fields.insert("name".into(), StdValue::String("Ada".into()));
        fields.insert("active".into(), StdValue::Bool(true));
        let s = Struct::new(fields);
        let bytes = s.encode().unwrap();
        let back = Struct::decode(&bytes).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn std_value_roundtrip() {
        let cases: Vec<StdValue> = vec![
            StdValue::Null(NullValue::NullValue),
            StdValue::Number(3.14),
            StdValue::String("hello".into()),
            StdValue::Bool(true),
            StdValue::Struct(Struct::default()),
            StdValue::List(ListValue::default()),
        ];
        for v in cases {
            let bytes = v.encode().unwrap();
            let back = StdValue::decode(&bytes).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn list_value_roundtrip() {
        let lv = ListValue::new(vec![
            StdValue::String("a".into()),
            StdValue::Number(1.0),
            StdValue::Bool(false),
        ]);
        let bytes = lv.encode().unwrap();
        let back = ListValue::decode(&bytes).unwrap();
        assert_eq!(lv, back);
    }

    #[test]
    fn field_mask_roundtrip() {
        let fm = FieldMask::new(vec!["name".into(), "email".into()]);
        let bytes = fm.encode().unwrap();
        let back = FieldMask::decode(&bytes).unwrap();
        assert_eq!(fm, back);
    }

    #[test]
    fn uuid_roundtrip() {
        let u = Uuid::new("550e8400-e29b-41d4-a716-446655440000".into());
        let bytes = u.encode().unwrap();
        let back = Uuid::decode(&bytes).unwrap();
        assert_eq!(u, back);
    }

    #[test]
    fn decimal_roundtrip() {
        let d = Decimal::new(b"\x19\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec());
        let bytes = d.encode().unwrap();
        let back = Decimal::decode(&bytes).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn money_roundtrip() {
        let m = Money::new("USD".into(), 10, 500_000_000);
        let bytes = m.encode().unwrap();
        let back = Money::decode(&bytes).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn interval_roundtrip() {
        let i = Interval::new(Timestamp::new(100, 0), Timestamp::new(200, 0));
        let bytes = i.encode().unwrap();
        let back = Interval::decode(&bytes).unwrap();
        assert_eq!(i, back);
    }

    #[test]
    fn pagination_roundtrip() {
        let p = Pagination::new("next".into(), Some(25), Some(100));
        let bytes = p.encode().unwrap();
        let back = Pagination::decode(&bytes).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn error_detail_roundtrip() {
        let e = ErrorDetail::new(
            "NOT_FOUND".into(),
            "user missing".into(),
            vec![Any::new("type.googleapis.com/foo".into(), b"detail".to_vec())],
        );
        let bytes = e.encode().unwrap();
        let back = ErrorDetail::decode(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn wrapper_bool_roundtrip() {
        let w = BoolValue { value: Some(true) };
        let bytes = w.encode().unwrap();
        let back = BoolValue::decode(&bytes).unwrap();
        assert_eq!(w, back);
        let empty = BoolValue { value: None };
        let bytes = empty.encode().unwrap();
        assert!(bytes.is_empty());
        let back = BoolValue::decode(&bytes).unwrap();
        assert_eq!(back, empty);
    }

    #[test]
    fn wrapper_string_roundtrip() {
        let w = StringValue {
            value: Some("hello".into()),
        };
        let bytes = w.encode().unwrap();
        let back = StringValue::decode(&bytes).unwrap();
        assert_eq!(w, back);
    }

    #[test]
    fn wrapper_int64_roundtrip() {
        let w = Int64Value { value: Some(-7) };
        let bytes = w.encode().unwrap();
        let back = Int64Value::decode(&bytes).unwrap();
        assert_eq!(w, back);
    }

    #[test]
    fn wrapper_double_roundtrip() {
        let w = DoubleValue { value: Some(2.5) };
        let bytes = w.encode().unwrap();
        let back = DoubleValue::decode(&bytes).unwrap();
        assert_eq!(w, back);
    }
}

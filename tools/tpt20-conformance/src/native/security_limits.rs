use tpt20_core::{DecoderLimits, DecodeError, Field, RawMessage, UnknownFieldPolicy, Value, WireClass};

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
fn field_count_limit_enforced() {
    let limits = DecoderLimits {
        max_field_count: 2,
        ..DecoderLimits::default()
    };
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    msg.push(Field::new(2, WireClass::Varint, Value::Varint(2)));
    msg.push(Field::new(3, WireClass::Varint, Value::Varint(3)));
    let bytes = msg.encode().unwrap();
    assert_eq!(
        RawMessage::decode(&bytes, &limits, UnknownFieldPolicy::Preserve),
        Err(DecodeError::FieldCountExceeded)
    );
}

#[test]
fn unknown_budget_is_enforced() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(200, WireClass::Len, Value::Len(vec![0u8; 32])));
    let bytes = msg.encode().unwrap();
    let limits = DecoderLimits {
        max_unknown_field_bytes: 8,
        ..DecoderLimits::default()
    };
    assert_eq!(
        RawMessage::decode_filtered(&bytes, &limits, UnknownFieldPolicy::Preserve, &|_| false),
        Err(DecodeError::LimitExceeded { limit: 8 })
    );
}

#[test]
fn known_fields_not_charged_to_unknown_budget() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Len, Value::Len(vec![0u8; 32])));
    let bytes = msg.encode().unwrap();
    let limits = DecoderLimits {
        max_unknown_field_bytes: 8,
        ..DecoderLimits::default()
    };
    assert!(RawMessage::decode_filtered(
        &bytes,
        &limits,
        UnknownFieldPolicy::Preserve,
        &|id| id == 1
    ).is_ok());
}

#[test]
fn default_limits_are_conservative() {
    let limits = DecoderLimits::default();
    assert!(limits.max_message_bytes >= 4 * 1024 * 1024);
    assert!(limits.max_depth >= 100);
    assert!(limits.max_field_count >= 32 * 1024);
}

#[test]
fn string_limit_is_enforced() {
    let limits = DecoderLimits::default();
    assert!(limits.check_string_bytes(1024).is_ok());
    let tight = DecoderLimits {
        max_string_bytes: 100,
        ..DecoderLimits::default()
    };
    assert!(tight.check_string_bytes(101).is_err());
}

#[test]
fn depth_limit_is_enforced() {
    let limits = DecoderLimits::default();
    assert!(limits.check_depth(1).is_ok());
    assert!(limits.check_depth(100).is_ok());
    assert!(limits.check_depth(101).is_err());
}

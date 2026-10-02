use proptest::prelude::*;
use tpt20_core::{DecoderLimits, Field, RawMessage, UnknownFieldPolicy, Value, WireClass};

proptest! {
    #[test]
    fn roundtrip_encode_decode(msg in arb_message()) {
        let bytes = msg.encode().unwrap();
        let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
        assert_eq!(msg.fields, back.fields);
    }

    #[test]
    fn roundtrip_decode_encode(msg in arb_message()) {
        let bytes = msg.encode().unwrap();
        let back = RawMessage::decode(&bytes, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
        let reencoded = back.encode().unwrap();
        let again = RawMessage::decode(&reencoded, &DecoderLimits::default(), UnknownFieldPolicy::Preserve).unwrap();
        assert_eq!(back.fields, again.fields);
    }
}

fn arb_message() -> impl Strategy<Value = RawMessage> {
    prop::collection::vec(arb_field(), 0..10).prop_map(|fields| RawMessage { fields })
}

fn arb_field() -> impl Strategy<Value = Field> {
    (1..100u32, arb_class_and_value()).prop_map(|(field_id, (wire_class, value))| Field {
        field_id,
        wire_class,
        value,
    })
}

/// Wire class and value are generated together so they always agree.
fn arb_class_and_value() -> impl Strategy<Value = (WireClass, Value)> {
    prop_oneof![
        any::<u64>().prop_map(|v| (WireClass::Varint, Value::Varint(v))),
        any::<u32>().prop_map(|v| (WireClass::Fixed32, Value::Fixed32(v))),
        any::<u64>().prop_map(|v| (WireClass::Fixed64, Value::Fixed64(v))),
        any::<Vec<u8>>().prop_map(|v| (WireClass::Len, Value::Len(v))),
    ]
}

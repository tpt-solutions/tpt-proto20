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
    (1..100u32, arb_wire_class(), arb_value()).prop_map(|(field_id, wire_class, value)| {
        Field { field_id, wire_class, value }
    })
}

fn arb_wire_class() -> impl Strategy<Value = WireClass> {
    prop_oneof![
        Just(WireClass::Varint),
        Just(WireClass::Fixed32),
        Just(WireClass::Fixed64),
        Just(WireClass::Len),
    ]
}

fn arb_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<u64>().prop_map(Value::Varint),
        any::<u32>().prop_map(Value::Fixed32),
        any::<u64>().prop_map(Value::Fixed64),
        any::<Vec<u8>>().prop_map(Value::Len),
    ]
}

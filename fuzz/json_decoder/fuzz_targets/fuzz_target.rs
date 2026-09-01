#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_core::{DecoderLimits, Field, FieldDescriptor, FieldKind, MessageDescriptor, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_core::ScalarKind;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let desc = make_desc(data[0] as u32);
    let payload = &data[1..];
    let _ = tpt20_core::DynamicMessage::decode_descriptor(desc, payload, &DecoderLimits::default());
});

fn make_desc(field_count: u32) -> MessageDescriptor {
    let mut desc = MessageDescriptor::new();
    for i in 1..=(field_count % 5 + 1) {
        desc.add_field(FieldDescriptor::new(
            i,
            format!("field_{}", i),
            WireClass::Varint,
            FieldKind::Scalar(ScalarKind::Int64),
        ));
    }
    desc
}

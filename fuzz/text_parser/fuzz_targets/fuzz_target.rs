#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_core::{DecoderLimits, Field, MessageDescriptor, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_core::ScalarKind;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let desc = make_desc(data[0] as u32);
    let payload = &data[1.min(data.len())..];
    let _ = tpt20_core::DynamicMessage::decode_descriptor(desc, payload, &DecoderLimits::default());

    // Also fuzz text format output generation (one-way)
    let mut msg = tpt20_core::DynamicMessage::new();
    for (i, byte) in data.iter().take(10).enumerate() {
        msg.set_varint((i + 1) as u32, *byte as u64);
    }
    let _ = msg.to_text();
});

fn make_desc(field_count: u32) -> MessageDescriptor {
    let mut desc = MessageDescriptor::new();
    for i in 1..=(field_count % 5 + 1) {
        desc.add_field(tpt20_core::FieldDescriptor::new(
            i,
            format!("field_{}", i),
            WireClass::Varint,
            tpt20_core::FieldKind::Scalar(ScalarKind::Int64),
        ));
    }
    desc
}

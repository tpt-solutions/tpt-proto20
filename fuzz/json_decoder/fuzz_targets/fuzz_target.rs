#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_core::{
    DynamicMessage, FieldDescriptor, FieldKind, MessageDescriptor, ScalarKind, WireClass,
};

fn descriptor() -> MessageDescriptor {
    let mut desc = MessageDescriptor::new();
    let fields = [
        (1, "id", WireClass::Varint, ScalarKind::Int64),
        (2, "name", WireClass::Len, ScalarKind::String),
        (3, "data", WireClass::Len, ScalarKind::Bytes),
        (4, "flag", WireClass::Varint, ScalarKind::Bool),
        (5, "ratio", WireClass::Fixed64, ScalarKind::Double),
        (6, "small", WireClass::Fixed32, ScalarKind::Fixed32),
        (7, "delta", WireClass::Varint, ScalarKind::Sint64),
    ];
    for (id, name, class, kind) in fields {
        desc.add_field(FieldDescriptor::new(
            id,
            name,
            class,
            FieldKind::Scalar(kind),
        ));
    }
    desc
}

fuzz_target!(|data: &[u8]| {
    let Ok(json) = std::str::from_utf8(data) else {
        return;
    };
    // JSON text decoding: arbitrary text must never panic, and anything
    // accepted must re-encode to JSON and to wire bytes without panicking.
    if let Ok(msg) = DynamicMessage::from_json(descriptor(), json) {
        let _ = msg.to_json();
        let _ = msg.encode();
        let _ = msg.to_text();
    }
});

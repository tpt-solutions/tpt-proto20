#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt20_core::{DecoderLimits, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_core::Field;
use tpt20_ir as ir;
use tpt20_descriptor::Descriptor;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let desc = make_descriptor(data[0]);
    let msg_ir = desc.find_message("FuzzMsg").unwrap();
    let payload = &data[1..];
    let _ = tpt20_reflect::DynamicMessage::decode(msg_ir, &desc, payload, &DecoderLimits::default(), UnknownFieldPolicy::Preserve);
});

fn make_descriptor(seed: u8) -> Descriptor {
    let pkg = ir::PackageIr {
        name: Some("fuzz.v1".to_string()),
        imports: vec![],
        messages: vec![ir::MessageIr {
            name: "FuzzMsg".into(),
            fields: vec![
                ir::FieldIr {
                    id: 1,
                    name: "id".into(),
                    label: ir::FieldLabelIr::Singular(ir::TypeRefIr { path: vec!["int64".into()] }),
                    presence: ir::Presence::Implicit,
                    annotations: vec![],
                    span: ir::SourceSpan::default(),
                },
                ir::FieldIr {
                    id: 2,
                    name: "name".into(),
                    label: ir::FieldLabelIr::Singular(ir::TypeRefIr { path: vec!["string".into()] }),
                    presence: ir::Presence::Implicit,
                    annotations: vec![],
                    span: ir::SourceSpan::default(),
                },
                ir::FieldIr {
                    id: 3 + (seed % 3) as u32,
                    name: "tag".into(),
                    label: ir::FieldLabelIr::Repeated(ir::TypeRefIr { path: vec!["int64".into()] }),
                    presence: ir::Presence::Implicit,
                    annotations: vec![],
                    span: ir::SourceSpan::default(),
                },
            ],
            oneofs: vec![],
            messages: vec![],
            enums: vec![],
            reserved: vec![],
            annotations: vec![],
            span: ir::SourceSpan::default(),
        }],
        enums: vec![],
        services: vec![],
        reserved: vec![],
        compat: ir::CompatMetadata::default(),
        fingerprint: None,
    };
    let mut desc = Descriptor::new(pkg);
    desc.compute_fingerprint();
    desc
}

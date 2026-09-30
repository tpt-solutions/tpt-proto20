#![no_main]

use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;
use tpt20_descriptor::Descriptor;
use tpt20_text::TextFormat;

const SCHEMA: &str = r#"
package fuzz.v1;
message Leaf { 1: v int64; 2: s string?; }
message Root {
  1: id int64;
  2: name string;
  3: tags repeated string;
  4: attrs map<string, Leaf>;
  oneof pick { 5: a string; 6: b Leaf; }
  enum E { Z = 0; O = 1; }
  7: e E;
  8: bytes_field bytes;
  9: f float64;
  10: nested Leaf;
}
"#;

fn descriptor() -> &'static Descriptor {
    static D: OnceLock<Descriptor> = OnceLock::new();
    D.get_or_init(|| {
        Descriptor::new(
            tpt20_compiler::compile(SCHEMA, None)
                .expect("fixture compiles")
                .ir,
        )
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let fmt = TextFormat::new(descriptor());
    // Parser must never panic; whatever it accepts must print and re-parse
    // to the same printed form (round-trip stability).
    if let Ok(raw) = fmt.parse("Root", text) {
        let printed = fmt.print("Root", &raw).expect("accepted input prints");
        let again = fmt.parse("Root", &printed).expect("printed text re-parses");
        assert_eq!(printed, fmt.print("Root", &again).unwrap());
    }
    // Printing arbitrary wire bytes must not panic either.
    let _ = fmt.print_bytes("Root", data);
});

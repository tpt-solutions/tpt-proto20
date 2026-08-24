//! Build script: compiles the fixture schema and generates real Rust code
//! into `OUT_DIR`, so the crate's tests compile and execute generator output.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let schema_path = manifest.join("src/schema.tpt");
    println!("cargo:rerun-if-changed=src/schema.tpt");
    let schema = fs::read_to_string(&schema_path).expect("read schema.tpt");

    let compiled = tpt20_compiler::compile(&schema, Some("schema.tpt"))
        .expect("fixture schema must compile cleanly");

    let opts = tpt20_codegen_rust::CodegenOptions {
        builders: true,
        ..Default::default()
    };
    let module = tpt20_codegen_rust::generate_module(&compiled.ir, &opts);

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("generated.rs"), module).expect("write generated.rs");
    fs::write(out_dir.join("fingerprint.txt"), &compiled.fingerprint)
        .expect("write fingerprint");
}

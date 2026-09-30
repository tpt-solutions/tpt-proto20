//! Generates Rust code for the benchmark schema into `OUT_DIR`.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=schema.tpt");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let schema = fs::read_to_string(manifest.join("schema.tpt")).unwrap();
    let compiled = tpt20_compiler::compile(&schema, Some("schema.tpt")).expect("schema compiles");
    let module = tpt20_codegen_rust::generate_module(
        &compiled.ir,
        &tpt20_codegen_rust::CodegenOptions::default(),
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out.join("generated.rs"), module).unwrap();
}

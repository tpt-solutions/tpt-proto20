//! Compiles `greeter.tpt` and generates Rust code into `OUT_DIR`
//! (the same thing `tpt20 gen rust --in greeter.tpt --out src/generated` does).

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=greeter.tpt");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = fs::read_to_string(manifest.join("greeter.tpt")).unwrap();
    let compiled = tpt20_compiler::compile(&src, Some("greeter.tpt")).expect("schema compiles");
    let module = tpt20_codegen_rust::generate_module(
        &compiled.ir,
        &tpt20_codegen_rust::CodegenOptions::default(),
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out.join("greeter.rs"), module).unwrap();
}

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

    // A second fixture enters through the `.proto` importer (edition 2023,
    // extensions) and takes the same code generation path.
    println!("cargo:rerun-if-changed=src/imported.proto");
    let proto = fs::read_to_string(manifest.join("src/imported.proto")).expect("read proto");
    let ast = tpt20_compat_protobuf::parse_proto(
        tpt20_compat_protobuf::lex_proto(&proto).expect("lex imported.proto"),
    )
    .expect("parse imported.proto");
    let ir = tpt20_compat_protobuf::lower(ast).expect("lower imported.proto");
    let module = tpt20_codegen_rust::generate_module(&ir, &opts);
    fs::write(out_dir.join("imported.rs"), module).expect("write imported.rs");
    fs::write(
        out_dir.join("imported_ir.json"),
        serde_json::to_string(&ir).expect("ir json"),
    )
    .expect("write imported_ir.json");
    fs::write(out_dir.join("fingerprint.txt"), &compiled.fingerprint).expect("write fingerprint");
}

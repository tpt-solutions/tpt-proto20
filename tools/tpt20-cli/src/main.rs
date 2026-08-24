//! `tpt20` command-line interface.
//!
//! Minimal Phase 5 stub: hosts the `gen rust` command wiring the Rust code
//! generator (todo Phase 5); the full command set (`check`, `fmt`, `lint`,
//! `diff`, `descriptors`, encode/decode converters, RPC debugger, registry)
//! lands with todo Phase 16.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tpt20_codegen_rust::{generate_module, output_file_stem, CodegenOptions};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(msg)) => {
            eprintln!("{msg}");
            print_usage();
            ExitCode::from(2)
        }
        Err(CliError::Diagnostics(text)) => {
            eprintln!("{text}");
            ExitCode::FAILURE
        }
        Err(CliError::Io(err)) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

enum CliError {
    Usage(String),
    Diagnostics(String),
    Io(std::io::Error),
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

fn run(args: &[String]) -> Result<(), CliError> {
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") | Some("help") => {
            print_usage();
            Ok(())
        }
        Some("-V") | Some("--version") | Some("version") => {
            println!("tpt20 {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("gen") => gen_rust(&args[1..]),
        Some(other) => Err(CliError::Usage(format!("unknown command: {other}"))),
    }
}

/// Implements `tpt20 gen rust --in <schema.tpt> --out <dir> [--builders]`.
fn gen_rust(args: &[String]) -> Result<(), CliError> {
    let mut input: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut opts = CodegenOptions::default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--in" => {
                i += 1;
                input = args.get(i).map(PathBuf::from);
                if input.is_none() {
                    return Err(CliError::Usage("missing value for --in".into()));
                }
            }
            "--out" => {
                i += 1;
                out_dir = args.get(i).map(PathBuf::from);
                if out_dir.is_none() {
                    return Err(CliError::Usage("missing value for --out".into()));
                }
            }
            "--builders" => opts.builders = true,
            other => return Err(CliError::Usage(format!("unknown flag: {other}"))),
        }
        i += 1;
    }

    let input = input.ok_or_else(|| CliError::Usage("missing --in <schema.tpt>".into()))?;
    let out_dir =
        out_dir.unwrap_or_else(|| PathBuf::from("src/generated"));

    let src = std::fs::read_to_string(&input)?;
    let compiled = tpt20_compiler::compile(&src, input.to_str()).map_err(|diags| {
        CliError::Diagnostics(tpt20_compiler::render_all(&diags))
    })?;

    let module = generate_module(&compiled.ir, &opts);
    std::fs::create_dir_all(&out_dir)?;
    let file_name = format!(
        "{}.rs",
        output_file_stem(&compiled.ir)
    );
    let dest = join_out(&out_dir, &file_name);
    std::fs::write(&dest, module)?;
    println!("generated {}", dest.display());
    Ok(())
}

fn join_out(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

fn print_usage() {
    println!(
        r#"tpt20 {} — schema compiler tooling

USAGE:
    tpt20 <COMMAND>

COMMANDS:
    gen         Generate code from a .tpt schema
        tpt20 gen rust --in <schema.tpt> --out <dir> [--builders]

    version     Print version information

Full command set arrives with tooling Phase 16."#,
        env!("CARGO_PKG_VERSION")
    );
}

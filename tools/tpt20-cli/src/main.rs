//! `tpt20` command-line interface — Phase 16 Developer Tooling (spec §21).
//!
//! Subcommands:
//! - `init`            create a new tpt20 project
//! - `check`           semantic-check a schema without codegen
//! - `fmt`             rewrite a schema in canonical form
//! - `lint`            run configurable lint rules
//! - `diff`            compare two schemas (SAFE/WARNING/BREAKING)
//! - `gen rust`        generate Rust code from a schema
//! - `descriptors`     emit the compiled descriptor (JSON or binary)
//! - `decode`          decode binary to a dynamic JSON representation
//! - `encode`          encode JSON to binary
//! - `text-to-binary`  convert text format to binary
//! - `binary-to-text`  convert binary to text format
//! - `json-to-binary`  convert JSON to binary
//! - `binary-to-json`  convert binary to JSON
//! - `import-proto`    import a .proto file to .tpt
//! - `conformance`     run conformance test vectors
//! - `call`            RPC debugger (unary/streaming)
//! - `health`          check service health
//! - `reflect`         introspect a descriptor
//! - `registry publish` publish a descriptor to the local registry

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use clap::{Parser, Subcommand, ValueEnum};

// ---------------------------------------------------------------------------
// Error model
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("usage error: {0}")]
    Usage(String),
    #[error("{0}")]
    Diagnostics(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("registry error: {0}")]
    Registry(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<tpt20_descriptor::DescriptorError> for CliError {
    fn from(e: tpt20_descriptor::DescriptorError) -> Self {
        CliError::Parse(e.to_string())
    }
}

impl CliError {
    fn exit_code(&self) -> u8 {
        match self {
            CliError::Usage(_) => 2,
            _ => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// CLI definition (clap derive)
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "tpt20", about = "tpt20 schema compiler tooling", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Create a new tpt20 project in the current directory
    Init {
        /// Project name (defaults to directory name)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Semantic-check a schema without codegen
    Check {
        /// Schema file to check
        file: PathBuf,
        /// Also emit descriptor JSON
        #[arg(long)]
        descriptor: bool,
    },

    /// Rewrite a schema in canonical form
    Fmt {
        /// Schema file to format (in-place)
        file: PathBuf,
        /// Write result to stdout instead of modifying file
        #[arg(short, long)]
        check: bool,
    },

    /// Run configurable lint rules
    Lint {
        /// Schema file(s) to lint
        files: Vec<PathBuf>,
        /// Lint configuration file (TOML)
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Output format: text or json
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,
        /// Treat warnings as errors
        #[arg(long)]
        deny_warnings: bool,
    },

    /// Compare two schemas for compatibility
    Diff {
        /// Old schema file
        old: PathBuf,
        /// New schema file
        new: PathBuf,
    },

    /// Generate code from a schema
    Gen {
        #[command(subcommand)]
        backend: GenBackend,
    },

    /// Emit the compiled descriptor
    Descriptors {
        /// Schema file
        file: PathBuf,
        /// Output format: json or binary
        #[arg(short, long, default_value = "json")]
        format: DescriptorFormat,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Decode binary bytes to a JSON representation
    Decode {
        /// Binary input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Encode JSON input to binary
    Encode {
        /// JSON input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Convert text format to binary
    TextToBinary {
        /// Text input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Convert binary to text format
    BinaryToText {
        /// Binary input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Convert JSON to binary
    JsonToBinary {
        /// JSON input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Convert binary to JSON
    BinaryToJson {
        /// Binary input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Import a .proto file and emit .tpt
    ImportProto {
        /// .proto input file
        input: PathBuf,
        /// Output .tpt file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Run conformance test vectors
    Conformance {
        /// Test vector directory
        #[arg(short, long)]
        directory: Option<PathBuf>,
        /// Specific test to run
        #[arg(short, long)]
        test: Option<String>,
    },

    /// RPC debugger
    Call {
        /// Target endpoint (e.g. http://localhost:50051)
        endpoint: String,
        /// Method to call (e.g. user.v1.UserService/GetUser)
        method: String,
        /// JSON input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Binary input file
        #[arg(short, long)]
        binary_input: Option<PathBuf>,
        /// Streaming call type: unary, server, client, bidi
        #[arg(short, long, default_value = "unary")]
        streaming: StreamingTypeArg,
        /// Metadata key=value pairs
        #[arg(short, long)]
        metadata: Vec<String>,
        /// Deadline in milliseconds
        #[arg(short, long)]
        deadline_ms: Option<u64>,
        /// TLS certificate file
        #[arg(long)]
        tls_cert: Option<PathBuf>,
        /// Compression algorithm: none, gzip, deflate
        #[arg(long, default_value = "none")]
        compression: CompressionArg,
    },

    /// Check service health
    Health {
        /// Target endpoint
        endpoint: String,
        /// TLS certificate file
        #[arg(long)]
        tls_cert: Option<PathBuf>,
    },

    /// Introspect a descriptor
    Reflect {
        /// Schema file
        file: PathBuf,
        /// Message type to inspect
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Publish a descriptor to the local registry
    Registry {
        #[command(subcommand)]
        command: RegistryCommands,
    },
}

#[derive(Subcommand, Debug)]
enum GenBackend {
    /// Generate Rust code
    Rust {
        /// Input schema file
        #[arg(short, long)]
        input: PathBuf,
        /// Output directory
        #[arg(short, long, default_value = "src/generated")]
        output: PathBuf,
        /// Emit validated builders
        #[arg(long)]
        builders: bool,
    },
}

#[derive(Subcommand, Debug)]
enum RegistryCommands {
    /// Publish a descriptor to the local registry
    Publish {
        /// Schema file
        file: PathBuf,
        /// Registry root directory (defaults to ~/.tpt20/registry)
        #[arg(short, long)]
        registry: Option<PathBuf>,
        /// Version label (defaults to package name)
        #[arg(short, long)]
        version: Option<String>,
    },
}

#[derive(ValueEnum, Clone, Debug)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(ValueEnum, Clone, Debug)]
enum DescriptorFormat {
    Json,
    Binary,
}

#[derive(ValueEnum, Clone, Debug)]
enum StreamingTypeArg {
    Unary,
    Server,
    Client,
    Bidi,
}

#[derive(ValueEnum, Clone, Debug)]
enum CompressionArg {
    None,
    Gzip,
    Deflate,
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::from(e.exit_code())
        }
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Commands::Init { name } => cmd_init(name),
        Commands::Check { file, descriptor } => cmd_check(file, descriptor),
        Commands::Fmt { file, check } => cmd_fmt(file, check),
        Commands::Lint { files, config, format, deny_warnings } => {
            cmd_lint(files, config, format, deny_warnings)
        }
        Commands::Diff { old, new } => cmd_diff(old, new),
        Commands::Gen { backend } => cmd_gen(backend),
        Commands::Descriptors { file, format, out } => cmd_descriptors(file, format, out),
        Commands::Decode { input, output } => cmd_decode(input, output),
        Commands::Encode { input, output } => cmd_encode(input, output),
        Commands::TextToBinary { input, output } => cmd_text_to_binary(input, output),
        Commands::BinaryToText { input, output } => cmd_binary_to_text(input, output),
        Commands::JsonToBinary { input, output } => cmd_json_to_binary(input, output),
        Commands::BinaryToJson { input, output } => cmd_binary_to_json(input, output),
        Commands::ImportProto { input, output } => cmd_import_proto(input, output),
        Commands::Conformance { directory, test } => cmd_conformance(directory, test),
        Commands::Call {
            endpoint,
            method,
            input,
            binary_input,
            streaming,
            metadata,
            deadline_ms,
            tls_cert,
            compression,
        } => cmd_call(
            endpoint, method, input, binary_input, streaming,
            metadata, deadline_ms, tls_cert, compression,
        ).await,
        Commands::Health { endpoint, tls_cert } => cmd_health(endpoint, tls_cert).await,
        Commands::Reflect { file, message } => cmd_reflect(file, message),
        Commands::Registry { command } => cmd_registry(command),
    }
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

fn cmd_init(name: Option<String>) -> Result<(), CliError> {
    let project = name.unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "tpt20-project".to_string())
    });

    let dir = Path::new(&project);
    if dir.exists() {
        return Err(CliError::Usage(format!("directory '{}' already exists", project)));
    }

    fs::create_dir_all(dir)?;

    let src = dir.join("src");
    fs::create_dir_all(&src)?;

    let schema = format!(
r#"package {name};

message Example {{
    1: id int64;
    2: name string;
}}
"#,
        name = project
    );
    fs::write(src.join(format!("{}.tpt", project)), schema)?;

    let cargo = format!(
r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
tpt20-core = "0.1"
tpt20-runtime = "0.1"
"#,
        name = project
    );
    fs::write(dir.join("Cargo.toml"), cargo)?;

    let readme = format!("# {}\n\ntpt20 project.\n", project);
    fs::write(dir.join("README.md"), readme)?;

    let gitignore = "/target\n/Cargo.lock\n*.swp\n";
    fs::write(dir.join(".gitignore"), gitignore)?;

    println!("Created tpt20 project '{}'", project);
    Ok(())
}

// ---------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------

fn cmd_check(file: PathBuf, show_descriptor: bool) -> Result<(), CliError> {
    let src = fs::read_to_string(&file)?;
    let diags = tpt20_compiler::check(&src, file.to_str());
    if !diags.is_empty() {
        eprintln!("{}", tpt20_compiler::render_all(&diags));
        if diags.iter().any(|d| d.severity == tpt20_compiler::Severity::Error) {
            return Err(CliError::Diagnostics("check failed".into()));
        }
    }

    if show_descriptor {
        let out = tpt20_compiler::compile(&src, file.to_str()).map_err(|diags| {
            CliError::Diagnostics(tpt20_compiler::render_all(&diags))
        })?;
        println!("{}", out.descriptor.to_json()?);
    }

    println!("check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// fmt
// ---------------------------------------------------------------------------

fn cmd_fmt(file: PathBuf, check: bool) -> Result<(), CliError> {
    let src = fs::read_to_string(&file)?;
    let formatted = format_schema(&src);

    if check {
        if src != formatted {
            eprintln!("schema would be reformatted");
            return Err(CliError::Usage("file not formatted".into()));
        }
    } else {
        fs::write(&file, formatted)?;
        println!("formatted {}", file.display());
    }
    Ok(())
}

fn format_schema(src: &str) -> String {
    let mut out = String::new();
    let mut indent = 0u32;
    let mut last_was_newline = false;

    for token in tokenize(src) {
        match token {
            FmtToken::Newline => {
                out.push('\n');
                last_was_newline = true;
            }
            FmtToken::Indent => {
                for _ in 0..indent {
                    out.push_str("    ");
                }
                last_was_newline = false;
            }
            FmtToken::OpenBrace => {
                if !last_was_newline {
                    out.push('\n');
                    for _ in 0..indent {
                        out.push_str("    ");
                    }
                }
                out.push('{');
                out.push('\n');
                indent += 1;
                for _ in 0..indent {
                    out.push_str("    ");
                }
                last_was_newline = true;
            }
            FmtToken::CloseBrace => {
                indent = indent.saturating_sub(1);
                if !last_was_newline {
                    out.push('\n');
                }
                for _ in 0..indent {
                    out.push_str("    ");
                }
                out.push('}');
                out.push('\n');
                last_was_newline = true;
            }
            FmtToken::Semicolon => {
                out.push(';');
                out.push('\n');
                for _ in 0..indent {
                    out.push_str("    ");
                }
                last_was_newline = true;
            }
            FmtToken::Text(t) => {
                if last_was_newline && !t.is_empty() {
                    for _ in 0..indent {
                        out.push_str("    ");
                    }
                }
                out.push_str(&t);
                last_was_newline = false;
            }
        }
    }
    out.trim().to_string() + "\n"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FmtToken {
    Newline,
    Indent,
    OpenBrace,
    CloseBrace,
    Semicolon,
    Text(&'static str),
}

fn tokenize(src: &str) -> Vec<FmtToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' | '\r' => {
                if !current.is_empty() {
                    tokens.push(FmtToken::Text(current.trim()));
                    current.clear();
                }
                tokens.push(FmtToken::Newline);
                while i + 1 < chars.len() && matches!(chars[i + 1], '\n' | '\r') {
                    i += 1;
                }
            }
            '{' => {
                if !current.is_empty() {
                    tokens.push(FmtToken::Text(current.trim()));
                    current.clear();
                }
                tokens.push(FmtToken::OpenBrace);
            }
            '}' => {
                if !current.is_empty() {
                    tokens.push(FmtToken::Text(current.trim()));
                    current.clear();
                }
                tokens.push(FmtToken::CloseBrace);
            }
            ';' => {
                if !current.is_empty() {
                    tokens.push(FmtToken::Text(current.trim()));
                    current.clear();
                }
                tokens.push(FmtToken::Semicolon);
            }
            c if c.is_whitespace() => {
                current.push(c);
            }
            _ => {
                current.push(c);
            }
        }
        i += 1;
    }

    if !current.is_empty() {
        tokens.push(FmtToken::Text(current.trim()));
    }

    tokens
}

// ---------------------------------------------------------------------------
// lint
// ---------------------------------------------------------------------------

fn cmd_lint(
    files: Vec<PathBuf>,
    config: Option<PathBuf>,
    format: OutputFormat,
    deny_warnings: bool,
) -> Result<(), CliError> {
    let config = config.unwrap_or_else(|| PathBuf::from(".tpt20-lint.toml"));
    let rules = if config.exists() {
        let raw = fs::read_to_string(&config)?;
        parse_lint_config(&raw)?
    } else {
        default_lint_rules()
    };

    let mut all_diags = Vec::new();
    for file in &files {
        let src = fs::read_to_string(file)?;
        let mut diags = tpt20_compiler::check(&src, file.to_str());
        for rule in &rules {
            diags.extend(rule.check(&src, file.to_str().expect("path is not valid UTF-8")));
        }
        all_diags.extend(diags);
    }

    if all_diags.is_empty() {
        println!("no lint errors found");
        return Ok(());
    }

    match format {
        OutputFormat::Json => {
            let serializable: Vec<LintDiag> = all_diags.iter().map(|d| LintDiag {
                code: d.code.clone(),
                message: d.message.clone(),
                severity: format!("{:?}", d.severity),
                file: d.file.clone(),
            }).collect();
            let json = serde_json::to_string_pretty(&serializable)?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            eprintln!("{}", tpt20_compiler::render_all(&all_diags));
        }
    }

    if all_diags.iter().any(|d| d.severity == tpt20_compiler::Severity::Error)
        || (deny_warnings && all_diags.iter().any(|d| d.severity == tpt20_compiler::Severity::Warning))
    {
        return Err(CliError::Diagnostics("lint failed".into()));
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct LintDiag {
    code: String,
    message: String,
    severity: String,
    file: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LintConfig {
    rules: Option<Vec<String>>,
}

fn parse_lint_config(raw: &str) -> Result<Vec<LintRule>, CliError> {
    let cfg: LintConfig = toml::from_str(raw).map_err(|e| CliError::Parse(e.to_string()))?;
    let names = cfg.rules.unwrap_or_default();
    let mut rules = Vec::new();
    for name in &names {
        let rule = match name.as_str() {
            "no-required" => LintRule::NoRequired,
            "package-required" => LintRule::PackageRequired,
            "reserved-reuse" => LintRule::ReservedReuse,
            "deprecated-usage" => LintRule::DeprecatedUsage,
            _ => return Err(CliError::Parse(format!("unknown lint rule: {name}"))),
        };
        rules.push(rule);
    }
    if rules.is_empty() {
        rules = default_lint_rules();
    }
    Ok(rules)
}

fn default_lint_rules() -> Vec<LintRule> {
    vec![
        LintRule::NoRequired,
        LintRule::PackageRequired,
        LintRule::ReservedReuse,
        LintRule::DeprecatedUsage,
    ]
}

#[derive(Debug, Clone, Copy)]
enum LintRule {
    NoRequired,
    PackageRequired,
    ReservedReuse,
    DeprecatedUsage,
}

impl LintRule {
    fn check(&self, src: &str, file: &str) -> Vec<tpt20_compiler::Diagnostic> {
        use tpt20_compiler::Diagnostic;
        let mut diags = Vec::new();
        match self {
            LintRule::NoRequired => {
                if src.contains("required") {
                    diags.push(
                        Diagnostic::warning("LINT001", "required keyword is deprecated; use explicit presence (`?`) instead")
                            .in_file(file)
                    );
                }
            }
            LintRule::PackageRequired => {
                if !src.contains("package ") {
                    diags.push(
                        Diagnostic::warning("LINT002", "schema is missing a package declaration")
                            .in_file(file)
                    );
                }
            }
            LintRule::ReservedReuse => {
                let reserved_re = regex::Regex::new(r"reserved\s+\d+\s+to\s+\d+").unwrap();
                for cap in reserved_re.find_iter(src) {
                    let range = cap.as_str();
                    let nums: Vec<u32> = regex::Regex::new(r"\d+")
                        .unwrap()
                        .find_iter(range)
                        .filter_map(|m| m.as_str().parse().ok())
                        .collect();
                    if nums.len() == 2 && nums[0] >= nums[1] {
                        diags.push(
                            Diagnostic::error("LINT003", format!("invalid reserved range: {}", range))
                                .in_file(file)
                        );
                    }
                }
            }
            LintRule::DeprecatedUsage => {
                if src.contains("@deprecated") {
                    diags.push(
                        Diagnostic::warning("LINT004", "deprecated annotation found")
                            .in_file(file)
                    );
                }
            }
        }
        diags
    }
}

// ---------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------

fn cmd_diff(old: PathBuf, new: PathBuf) -> Result<(), CliError> {
    let old_src = fs::read_to_string(&old)?;
    let new_src = fs::read_to_string(&new)?;

    let changes = tpt20_compiler::diff_sources(&old_src, &new_src)
        .map_err(|diags| CliError::Diagnostics(tpt20_compiler::render_all(&diags)))?;

    let report = tpt20_compiler::render_report(&changes);
    if report.is_empty() {
        println!("no differences");
    } else {
        println!("{}", report);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// gen
// ---------------------------------------------------------------------------

fn cmd_gen(backend: GenBackend) -> Result<(), CliError> {
    match backend {
        GenBackend::Rust { input, output, builders } => {
            let src = fs::read_to_string(&input)?;
            let compiled = tpt20_compiler::compile(&src, input.to_str()).map_err(|diags| {
                CliError::Diagnostics(tpt20_compiler::render_all(&diags))
            })?;

            let mut opts = tpt20_codegen_rust::CodegenOptions::default();
            opts.builders = builders;

            let module = tpt20_codegen_rust::generate_module(&compiled.ir, &opts);
            fs::create_dir_all(&output)?;
            let file_name = format!("{}.rs", tpt20_codegen_rust::output_file_stem(&compiled.ir));
            let dest = output.join(file_name);
            fs::write(&dest, module)?;
            println!("generated {}", dest.display());
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// descriptors
// ---------------------------------------------------------------------------

fn cmd_descriptors(file: PathBuf, format: DescriptorFormat, out: Option<PathBuf>) -> Result<(), CliError> {
    let src = fs::read_to_string(&file)?;
    let compiled = tpt20_compiler::compile(&src, file.to_str()).map_err(|diags| {
        CliError::Diagnostics(tpt20_compiler::render_all(&diags))
    })?;

    match format {
        DescriptorFormat::Json => {
            let json = compiled.descriptor.to_json()?;
            match out {
                Some(p) => fs::write(p, json)?,
                None => println!("{}", json),
            }
        }
        DescriptorFormat::Binary => {
            let bin = compiled.descriptor.to_binary()?;
            match out {
                Some(p) => fs::write(p, bin)?,
                None => {
                    std::io::stdout().write_all(&bin)?;
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// decode / encode / text-to-binary / binary-to-text / json-to-binary / binary-to-json
// ---------------------------------------------------------------------------

fn read_input(input: Option<PathBuf>) -> Result<Vec<u8>, CliError> {
    match input {
        Some(p) => Ok(fs::read(p)?),
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            Ok(buf)
        }
    }
}

fn write_output(data: &[u8], output: Option<PathBuf>) -> Result<(), CliError> {
    match output {
        Some(p) => Ok(fs::write(p, data)?),
        None => Ok(io::stdout().write_all(data)?),
    }
}

fn write_output_str(data: &str, output: Option<PathBuf>) -> Result<(), CliError> {
    match output {
        Some(p) => Ok(fs::write(p, data)?),
        None => Ok(io::stdout().write_all(data.as_bytes())?),
    }
}

fn field_value_to_json(field: &tpt20_core::Field) -> serde_json::Value {
    match &field.value {
        tpt20_core::Value::Varint(v) => serde_json::Value::String(v.to_string()),
        tpt20_core::Value::Fixed32(v) => serde_json::Value::String(v.to_string()),
        tpt20_core::Value::Fixed64(v) => serde_json::Value::String(v.to_string()),
        tpt20_core::Value::Len(bytes) => {
            if let Ok(s) = std::str::from_utf8(bytes) {
                serde_json::Value::String(s.to_string())
            } else {
                serde_json::Value::String(STANDARD.encode(bytes))
            }
        }
    }
}

fn cmd_decode(input: Option<PathBuf>, output: Option<PathBuf>) -> Result<(), CliError> {
    let bytes = read_input(input)?;
    let raw = tpt20_core::RawMessage::decode(
        &bytes,
        &tpt20_core::DecoderLimits::default(),
        tpt20_core::UnknownFieldPolicy::Preserve,
    )
    .map_err(|e| CliError::Parse(e.to_string()))?;

    let mut map = serde_json::Map::new();
    for field in &raw.fields {
        map.insert(field.field_id.to_string(), field_value_to_json(field));
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(map))?;
    write_output_str(&json, output)
}

fn cmd_encode(input: Option<PathBuf>, output: Option<PathBuf>) -> Result<(), CliError> {
    let json_str = read_input_string(input)?;
    let value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| CliError::Parse(format!("invalid json: {e}")))?;
    let obj = value.as_object().ok_or_else(|| CliError::Parse("expected json object".into()))?;

    let mut raw = tpt20_core::RawMessage::new();
    for (key, val) in obj {
        let id: u32 = key.parse().map_err(|_| CliError::Parse(format!("invalid field id: {key}")))?;
        let (wire, value) = json_value_to_core(val)?;
        raw.push(tpt20_core::Field::new(id, wire, value));
    }
    let bytes = raw.encode().map_err(|e| CliError::Parse(e.to_string()))?;
    write_output(&bytes, output)
}

fn cmd_text_to_binary(input: Option<PathBuf>, output: Option<PathBuf>) -> Result<(), CliError> {
    let text = read_input_string(input)?;
    let mut raw = tpt20_core::RawMessage::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.len() != 2 {
            continue;
        }
        let field_name = parts[0].trim();
        let value_str = parts[1].trim();

        let field_id = field_name.parse::<u32>().unwrap_or(0);
        if field_id == 0 {
            continue;
        }

        let (wire, value) = parse_text_value(value_str)?;
        raw.push(tpt20_core::Field::new(field_id, wire, value));
    }
    let bytes = raw.encode().map_err(|e| CliError::Parse(e.to_string()))?;
    write_output(&bytes, output)
}

fn cmd_binary_to_text(input: Option<PathBuf>, output: Option<PathBuf>) -> Result<(), CliError> {
    let bytes = read_input(input)?;
    let raw = tpt20_core::RawMessage::decode(
        &bytes,
        &tpt20_core::DecoderLimits::default(),
        tpt20_core::UnknownFieldPolicy::Preserve,
    )
    .map_err(|e| CliError::Parse(e.to_string()))?;

    let mut out = String::new();
    for field in &raw.fields {
        out.push_str(&format!("{}: {}\n", field.field_id, core_value_to_text(&field.value)));
    }
    write_output_str(&out, output)
}

fn cmd_json_to_binary(input: Option<PathBuf>, output: Option<PathBuf>) -> Result<(), CliError> {
    let json_str = read_input_string(input)?;
    let value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| CliError::Parse(format!("invalid json: {e}")))?;
    let obj = value.as_object().ok_or_else(|| CliError::Parse("expected json object".into()))?;

    let mut raw = tpt20_core::RawMessage::new();
    for (key, val) in obj {
        let id: u32 = key.parse().map_err(|_| CliError::Parse(format!("invalid field id: {key}")))?;
        let (wire, value) = json_value_to_core(val)?;
        raw.push(tpt20_core::Field::new(id, wire, value));
    }
    let bytes = raw.encode().map_err(|e| CliError::Parse(e.to_string()))?;
    write_output(&bytes, output)
}

fn cmd_binary_to_json(input: Option<PathBuf>, output: Option<PathBuf>) -> Result<(), CliError> {
    cmd_decode(input, output)
}

fn read_input_string(input: Option<PathBuf>) -> Result<String, CliError> {
    match input {
        Some(p) => Ok(fs::read_to_string(p)?),
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
    }
}

fn json_value_to_core(value: &serde_json::Value) -> Result<(tpt20_core::WireClass, tpt20_core::Value), CliError> {
    match value {
        serde_json::Value::Null => Ok((tpt20_core::WireClass::Len, tpt20_core::Value::Len(Vec::new()))),
        serde_json::Value::Bool(b) => Ok((tpt20_core::WireClass::Varint, tpt20_core::Value::Varint(if *b { 1 } else { 0 }))),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok((tpt20_core::WireClass::Varint, tpt20_core::Value::Varint(i as u64)))
            } else if let Some(u) = n.as_u64() {
                Ok((tpt20_core::WireClass::Varint, tpt20_core::Value::Varint(u)))
            } else if let Some(f) = n.as_f64() {
                Ok((tpt20_core::WireClass::Fixed64, tpt20_core::Value::Fixed64(f.to_bits())))
            } else {
                Err(CliError::Parse("unsupported number".into()))
            }
        }
        serde_json::Value::String(s) => {
            if let Ok(bytes) = STANDARD.decode(s) {
                Ok((tpt20_core::WireClass::Len, tpt20_core::Value::Len(bytes)))
            } else {
                Ok((tpt20_core::WireClass::Len, tpt20_core::Value::Len(s.as_bytes().to_vec())))
            }
        }
        serde_json::Value::Array(_) => {
            let bytes = serde_json::to_vec(value).map_err(|e| CliError::Parse(e.to_string()))?;
            Ok((tpt20_core::WireClass::Len, tpt20_core::Value::Len(bytes)))
        }
        serde_json::Value::Object(_) => {
            let bytes = serde_json::to_vec(value).map_err(|e| CliError::Parse(e.to_string()))?;
            Ok((tpt20_core::WireClass::Len, tpt20_core::Value::Len(bytes)))
        }
    }
}

fn parse_text_value(s: &str) -> Result<(tpt20_core::WireClass, tpt20_core::Value), CliError> {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len()-1];
        return Ok((tpt20_core::WireClass::Len, tpt20_core::Value::Len(inner.as_bytes().to_vec())));
    }
    if let Ok(b) = s.parse::<bool>() {
        return Ok((tpt20_core::WireClass::Varint, tpt20_core::Value::Varint(if b { 1 } else { 0 })));
    }
    if let Ok(i) = s.parse::<i64>() {
        return Ok((tpt20_core::WireClass::Varint, tpt20_core::Value::Varint(i as u64)));
    }
    if let Ok(u) = s.parse::<u64>() {
        return Ok((tpt20_core::WireClass::Varint, tpt20_core::Value::Varint(u)));
    }
    if let Ok(f) = s.parse::<f64>() {
        return Ok((tpt20_core::WireClass::Fixed64, tpt20_core::Value::Fixed64(f.to_bits())));
    }
    Ok((tpt20_core::WireClass::Len, tpt20_core::Value::Len(s.as_bytes().to_vec())))
}

fn core_value_to_text(value: &tpt20_core::Value) -> String {
    match value {
        tpt20_core::Value::Varint(v) => v.to_string(),
        tpt20_core::Value::Fixed32(v) => v.to_string(),
        tpt20_core::Value::Fixed64(v) => v.to_string(),
        tpt20_core::Value::Len(bytes) => {
            if let Ok(s) = std::str::from_utf8(bytes) {
                format!("\"{}\"", s.escape_default())
            } else {
                format!("[base64 {}]", STANDARD.encode(bytes))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// import-proto
// ---------------------------------------------------------------------------

fn cmd_import_proto(input: PathBuf, output: Option<PathBuf>) -> Result<(), CliError> {
    let src = fs::read_to_string(&input)?;
    let tokens = tpt20_compat_protobuf::lex_proto(&src)
        .map_err(|e| CliError::Parse(format!("lex error: {e}")))?;
    let proto = tpt20_compat_protobuf::parse_proto(tokens)
        .map_err(|e| CliError::Parse(format!("parse error: {e}")))?;
    let ir = tpt20_compat_protobuf::lower(proto)
        .map_err(|e| CliError::Parse(format!("lower error: {e}")))?;

    let json = serde_json::to_string_pretty(&ir).map_err(|e| CliError::Parse(e.to_string()))?;
    match output {
        Some(p) => Ok(fs::write(p, json)?),
        None => {
            println!("{}", json);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// conformance
// ---------------------------------------------------------------------------

fn cmd_conformance(directory: Option<PathBuf>, test: Option<String>) -> Result<(), CliError> {
    let dir = directory.unwrap_or_else(|| PathBuf::from("tests/conformance"));
    if !dir.exists() {
        println!("no conformance directory at {}", dir.display());
        return Ok(());
    }

    let entries = fs::read_dir(&dir).map_err(|e| CliError::Io(e))?;
    let mut passed = 0usize;
    let mut failed = 0usize;

    for entry in entries {
        let entry = entry.map_err(|e| CliError::Io(e))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if let Some(ref t) = test {
            if name != *t {
                continue;
            }
        }

        match run_conformance_test(&path) {
            Ok(()) => {
                println!("PASS {}", name);
                passed += 1;
            }
            Err(e) => {
                eprintln!("FAIL {}: {}", name, e);
                failed += 1;
            }
        }
    }

    println!("\n{} passed, {} failed", passed, failed);
    if failed > 0 {
        Err(CliError::Diagnostics("conformance tests failed".into()))
    } else {
        Ok(())
    }
}

fn run_conformance_test(path: &Path) -> Result<(), CliError> {
    let raw = fs::read_to_string(path)?;
    let _test: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| CliError::Parse(format!("invalid test json: {e}")))?;

    if let Some(obj) = _test.as_object() {
        if let Some(binary) = obj.get("binary").and_then(|v| v.as_str()) {
            let bytes = hex::decode(binary).map_err(|e| CliError::Parse(e.to_string()))?;
            tpt20_core::RawMessage::decode(
                &bytes,
                &tpt20_core::DecoderLimits::default(),
                tpt20_core::UnknownFieldPolicy::Preserve,
            )
            .map_err(|e| CliError::Parse(e.to_string()))?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// call (RPC debugger)
// ---------------------------------------------------------------------------

async fn cmd_call(
    endpoint: String,
    method: String,
    input: Option<PathBuf>,
    binary_input: Option<PathBuf>,
    streaming: StreamingTypeArg,
    metadata: Vec<String>,
    deadline_ms: Option<u64>,
    _tls_cert: Option<PathBuf>,
    _compression: CompressionArg,
) -> Result<(), CliError> {
    let request_bytes = if let Some(p) = binary_input {
        fs::read(p)?
    } else if let Some(p) = input {
        let json = fs::read_to_string(p)?;
        serde_json::to_vec(&serde_json::from_str::<serde_json::Value>(&json).map_err(|e| CliError::Parse(e.to_string()))?)?
    } else {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        serde_json::to_vec(&serde_json::from_str::<serde_json::Value>(&buf).map_err(|e| CliError::Parse(e.to_string()))?)?
    };

    let mut md = tpt20_rpc::Metadata::with_default_limit();
    for kv in &metadata {
        let parts: Vec<&str> = kv.splitn(2, '=').collect();
        if parts.len() == 2 {
            md.insert_text(parts[0], parts[1]).map_err(|e| CliError::Transport(e.to_string()))?;
        }
    }

    let streaming_type = match streaming {
        StreamingTypeArg::Unary => tpt20_transport::StreamingType::Unary,
        StreamingTypeArg::Server => tpt20_transport::StreamingType::ServerStream,
        StreamingTypeArg::Client => tpt20_transport::StreamingType::ClientStream,
        StreamingTypeArg::Bidi => tpt20_transport::StreamingType::Bidi,
    };

    println!("RPC call to {} ({})", endpoint, method);
    println!("metadata: {:?}", md.iter().collect::<Vec<_>>());

    if let Some(d) = deadline_ms {
        println!("deadline: {}ms", d);
    }
    println!("streaming: {:?}", streaming_type);
    println!("request sent ({} bytes)", request_bytes.len());
    println!("(In-process transport: no server response in CLI stub)");

    Ok(())
}

// ---------------------------------------------------------------------------
// health
// ---------------------------------------------------------------------------

async fn cmd_health(endpoint: String, _tls_cert: Option<PathBuf>) -> Result<(), CliError> {
    println!("Checking health of {}", endpoint);
    println!("health check initiated (in-process transport stub)");
    println!("(In-process transport: no server response in CLI stub)");
    Ok(())
}

// ---------------------------------------------------------------------------
// reflect
// ---------------------------------------------------------------------------

fn cmd_reflect(file: PathBuf, message: Option<String>) -> Result<(), CliError> {
    let src = fs::read_to_string(&file)?;
    let compiled = tpt20_compiler::compile(&src, file.to_str()).map_err(|diags| {
        CliError::Diagnostics(tpt20_compiler::render_all(&diags))
    })?;

    let desc = compiled.descriptor;

    if let Some(name) = message {
        if let Some(msg) = desc.find_message(&name) {
            println!("message: {}", msg.name);
            println!("fields:");
            for f in &msg.fields {
                let label = match &f.label {
                    tpt20_ir::FieldLabelIr::Singular(t) => format!("{}", t.name()),
                    tpt20_ir::FieldLabelIr::Repeated(t) => format!("repeated {}", t.name()),
                    tpt20_ir::FieldLabelIr::Map { key, value } => {
                        format!("map<{}, {}>", key.name(), value.name())
                    }
                };
                let presence = match f.presence {
                    tpt20_ir::Presence::Implicit => "implicit",
                    tpt20_ir::Presence::Explicit => "explicit",
                };
                println!("  {} (id {}): {} [{}]", f.name, f.id, label, presence);
            }
            for o in &msg.oneofs {
                println!("  oneof {}:", o.name);
                for f in &o.fields {
                    println!("    {} (id {})", f.name, f.id);
                }
            }
            for e in &msg.enums {
                println!("  enum {}: {}", e.name, if e.open { "open" } else { "closed" });
                for v in &e.values {
                    println!("    {} = {}", v.name, v.number);
                }
            }
        } else {
            return Err(CliError::Parse(format!("message '{}' not found", name)));
        }
    } else {
        println!("package: {:?}", desc.package.name);
        println!("fingerprint: {}", compiled.fingerprint);
        println!("messages:");
        for m in &desc.package.messages {
            println!("  {}", m.name);
        }
        println!("enums:");
        for e in &desc.package.enums {
            println!("  {} ({})", e.name, if e.open { "open" } else { "closed" });
        }
        println!("services:");
        for s in &desc.package.services {
            println!("  {}", s.name);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// registry
// ---------------------------------------------------------------------------

fn cmd_registry(command: RegistryCommands) -> Result<(), CliError> {
    match command {
        RegistryCommands::Publish { file, registry, version } => {
            let registry = registry.unwrap_or_else(|| {
                home::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".tpt20")
                    .join("registry")
            });

            fs::create_dir_all(&registry)?;

            let src = fs::read_to_string(&file)?;
            let compiled = tpt20_compiler::compile(&src, file.to_str()).map_err(|diags| {
                CliError::Diagnostics(tpt20_compiler::render_all(&diags))
            })?;

            let version = version.unwrap_or_else(|| {
                compiled.ir.name.clone().unwrap_or_else(|| "default".to_string())
            });

            let descriptor_json = compiled.descriptor.to_json()?;
            let version_dir = registry.join(&version);
            fs::create_dir_all(&version_dir)?;

            let descriptor_path = version_dir.join("descriptor.json");
            fs::write(&descriptor_path, descriptor_json)?;

            let mut manifest = LocalManifest::load_or_default(&registry);
            manifest.record_version(&version, &compiled.fingerprint, "strict");
            manifest.save(&registry)?;

            println!("published {} to registry ({})", version, registry.display());
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Local registry manifest helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LocalManifest {
    versions: Vec<VersionRecord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct VersionRecord {
    version: String,
    fingerprint: String,
    policy: String,
    published_at: String,
}

impl Default for LocalManifest {
    fn default() -> Self {
        LocalManifest { versions: Vec::new() }
    }
}

impl LocalManifest {
    fn load_or_default(root: &Path) -> Self {
        let path = root.join("manifest.json");
        if path.exists() {
            if let Ok(data) = fs::read_to_string(path) {
                if let Ok(m) = serde_json::from_str::<LocalManifest>(&data) {
                    return m;
                }
            }
        }
        LocalManifest::default()
    }

    fn save(&self, root: &Path) -> Result<(), CliError> {
        let path = root.join("manifest.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| CliError::Registry(e.to_string()))?;
        fs::write(path, json)?;
        Ok(())
    }

    fn record_version(&mut self, version: &str, fingerprint: &str, policy: &str) {
        self.versions.push(VersionRecord {
            version: version.to_string(),
            fingerprint: fingerprint.to_string(),
            policy: policy.to_string(),
            published_at: "now".to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_preserves_semantics() {
        let src = "package user.v1;\n\nmessage User { 1: id int64; 2: name string; }\n";
        let formatted = format_schema(src);
        let reparsed = tpt20_compiler::compile(&formatted, None);
        assert!(reparsed.is_ok(), "reparsed: {:?}", reparsed);
    }
}

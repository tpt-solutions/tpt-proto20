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

    /// Decode binary bytes (schema-free JSON, or text format with --schema)
    Decode {
        /// Binary input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Schema file; with --message, prints schema-aware text format
        #[arg(short, long, requires = "message")]
        schema: Option<PathBuf>,
        /// Message type to decode as (e.g. `User` or `Outer.Child`)
        #[arg(short, long, requires = "schema")]
        message: Option<String>,
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

    /// Convert text format to binary (schema-driven)
    TextToBinary {
        /// Text input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Schema file
        #[arg(short, long)]
        schema: PathBuf,
        /// Message type (e.g. `User` or `Outer.Child`)
        #[arg(short, long)]
        message: String,
    },

    /// Convert binary to text format (schema-driven)
    BinaryToText {
        /// Binary input file (defaults to stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Schema file
        #[arg(short, long)]
        schema: PathBuf,
        /// Message type (e.g. `User` or `Outer.Child`)
        #[arg(short, long)]
        message: String,
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
        /// JSON input file: a field-id-keyed object, or an array of them for
        /// client/bidi streams (defaults to stdin when no other input is given)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Binary input file (one message)
        #[arg(short, long)]
        binary_input: Option<PathBuf>,
        /// Text-format input file (needs --schema and --request-type)
        #[arg(long, requires_all = ["schema", "request_type"])]
        text_input: Option<PathBuf>,
        /// Schema file used for text input/output
        #[arg(long)]
        schema: Option<PathBuf>,
        /// Request message type (for --text-input)
        #[arg(long, requires = "schema")]
        request_type: Option<String>,
        /// Response message type; responses print as text format
        #[arg(long, requires = "schema")]
        response_type: Option<String>,
        /// Streaming call type: unary, server, client, bidi
        #[arg(short, long, default_value = "unary")]
        streaming: StreamingTypeArg,
        /// Metadata key=value pairs
        #[arg(short, long)]
        metadata: Vec<String>,
        /// Deadline in milliseconds
        #[arg(short, long)]
        deadline_ms: Option<u64>,
        /// CA certificate (PEM) to trust; enables TLS
        #[arg(long)]
        tls_cert: Option<PathBuf>,
        /// Compression algorithm: none (others are not supported yet)
        #[arg(long, default_value = "none")]
        compression: CompressionArg,
    },

    /// Check service health (`tpt20.health.v1.Health/Check`)
    Health {
        /// Target endpoint
        endpoint: String,
        /// Service name to check (empty = overall server status)
        #[arg(short, long, default_value = "")]
        service: String,
        /// Deadline in milliseconds
        #[arg(short, long, default_value_t = 5000)]
        deadline_ms: u64,
        /// CA certificate (PEM) to trust; enables TLS
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
        #[arg(short, long = "in")]
        input: PathBuf,
        /// Output directory
        #[arg(short, long = "out", default_value = "src/generated")]
        output: PathBuf,
        /// Emit validated builders
        #[arg(long)]
        builders: bool,
        /// Do not generate service traits/clients (skips the `tpt20-rpc` dependency)
        #[arg(long)]
        no_services: bool,
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
        /// Overwrite an already published version whose contents differ
        #[arg(long)]
        force: bool,
    },

    /// List published versions
    List {
        /// Registry root directory (defaults to ~/.tpt20/registry)
        #[arg(short, long)]
        registry: Option<PathBuf>,
    },

    /// Fetch a published descriptor by version label or fingerprint
    Get {
        /// Version label, or (a prefix of at least 8 characters of) a fingerprint
        version: String,
        /// Registry root directory (defaults to ~/.tpt20/registry)
        #[arg(short, long)]
        registry: Option<PathBuf>,
        /// Output format: json or binary
        #[arg(short, long, default_value = "json")]
        format: DescriptorFormat,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        out: Option<PathBuf>,
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
        Commands::Lint {
            files,
            config,
            format,
            deny_warnings,
        } => cmd_lint(files, config, format, deny_warnings),
        Commands::Diff { old, new } => cmd_diff(old, new),
        Commands::Gen { backend } => cmd_gen(backend),
        Commands::Descriptors { file, format, out } => cmd_descriptors(file, format, out),
        Commands::Decode {
            input,
            output,
            schema,
            message,
        } => cmd_decode(input, output, schema, message),
        Commands::Encode { input, output } => cmd_encode(input, output),
        Commands::TextToBinary {
            input,
            output,
            schema,
            message,
        } => cmd_text_to_binary(input, output, schema, message),
        Commands::BinaryToText {
            input,
            output,
            schema,
            message,
        } => cmd_binary_to_text(input, output, schema, message),
        Commands::JsonToBinary { input, output } => cmd_json_to_binary(input, output),
        Commands::BinaryToJson { input, output } => cmd_binary_to_json(input, output),
        Commands::ImportProto { input, output } => cmd_import_proto(input, output),
        Commands::Conformance { directory, test } => cmd_conformance(directory, test),
        Commands::Call {
            endpoint,
            method,
            input,
            binary_input,
            text_input,
            schema,
            request_type,
            response_type,
            streaming,
            metadata,
            deadline_ms,
            tls_cert,
            compression,
        } => {
            cmd_call(CallOpts {
                endpoint,
                method,
                input,
                binary_input,
                text_input,
                schema,
                request_type,
                response_type,
                streaming,
                metadata,
                deadline_ms,
                tls_cert,
                compression,
            })
            .await
        }
        Commands::Health {
            endpoint,
            service,
            deadline_ms,
            tls_cert,
        } => cmd_health(endpoint, service, deadline_ms, tls_cert).await,
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
        return Err(CliError::Usage(format!(
            "directory '{}' already exists",
            project
        )));
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
        if diags
            .iter()
            .any(|d| d.severity == tpt20_compiler::Severity::Error)
        {
            return Err(CliError::Diagnostics("check failed".into()));
        }
    }

    if show_descriptor {
        let out = tpt20_compiler::compile(&src, file.to_str())
            .map_err(|diags| CliError::Diagnostics(tpt20_compiler::render_all(&diags)))?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum FmtToken {
    Newline,
    Indent,
    OpenBrace,
    CloseBrace,
    Semicolon,
    Text(String),
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
                    tokens.push(FmtToken::Text(current.trim().to_string()));
                    current.clear();
                }
                tokens.push(FmtToken::Newline);
                while i + 1 < chars.len() && matches!(chars[i + 1], '\n' | '\r') {
                    i += 1;
                }
            }
            '{' => {
                if !current.is_empty() {
                    tokens.push(FmtToken::Text(current.trim().to_string()));
                    current.clear();
                }
                tokens.push(FmtToken::OpenBrace);
            }
            '}' => {
                if !current.is_empty() {
                    tokens.push(FmtToken::Text(current.trim().to_string()));
                    current.clear();
                }
                tokens.push(FmtToken::CloseBrace);
            }
            ';' => {
                if !current.is_empty() {
                    tokens.push(FmtToken::Text(current.trim().to_string()));
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
        tokens.push(FmtToken::Text(current.trim().to_string()));
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
            let serializable: Vec<LintDiag> = all_diags
                .iter()
                .map(|d| LintDiag {
                    code: d.code.to_string(),
                    message: d.message.clone(),
                    severity: format!("{:?}", d.severity),
                    file: d.file.clone(),
                })
                .collect();
            let json = serde_json::to_string_pretty(&serializable)?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            eprintln!("{}", tpt20_compiler::render_all(&all_diags));
        }
    }

    if all_diags
        .iter()
        .any(|d| d.severity == tpt20_compiler::Severity::Error)
        || (deny_warnings
            && all_diags
                .iter()
                .any(|d| d.severity == tpt20_compiler::Severity::Warning))
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
                        Diagnostic::warning(
                            "LINT001",
                            "required keyword is deprecated; use explicit presence (`?`) instead",
                        )
                        .in_file(file),
                    );
                }
            }
            LintRule::PackageRequired => {
                if !src.contains("package ") {
                    diags.push(
                        Diagnostic::warning("LINT002", "schema is missing a package declaration")
                            .in_file(file),
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
                            Diagnostic::error(
                                "LINT003",
                                format!("invalid reserved range: {}", range),
                            )
                            .in_file(file),
                        );
                    }
                }
            }
            LintRule::DeprecatedUsage => {
                if src.contains("@deprecated") {
                    diags.push(
                        Diagnostic::warning("LINT004", "deprecated annotation found").in_file(file),
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
        GenBackend::Rust {
            input,
            output,
            builders,
            no_services,
        } => {
            let src = fs::read_to_string(&input)?;
            let compiled = tpt20_compiler::compile(&src, input.to_str())
                .map_err(|diags| CliError::Diagnostics(tpt20_compiler::render_all(&diags)))?;

            let mut opts = tpt20_codegen_rust::CodegenOptions::default();
            opts.builders = builders;
            opts.services = !no_services;

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

fn cmd_descriptors(
    file: PathBuf,
    format: DescriptorFormat,
    out: Option<PathBuf>,
) -> Result<(), CliError> {
    let src = fs::read_to_string(&file)?;
    let compiled = tpt20_compiler::compile(&src, file.to_str())
        .map_err(|diags| CliError::Diagnostics(tpt20_compiler::render_all(&diags)))?;

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

fn load_descriptor(path: &Path) -> Result<tpt20_descriptor::Descriptor, CliError> {
    let src = fs::read_to_string(path)?;
    let compiled = tpt20_compiler::compile(&src, path.to_str())
        .map_err(|diags| CliError::Diagnostics(tpt20_compiler::render_all(&diags)))?;
    Ok(tpt20_descriptor::Descriptor::new(compiled.ir))
}

fn text_err(e: tpt20_text::TextError) -> CliError {
    CliError::Parse(e.to_string())
}

fn cmd_decode(
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    schema: Option<PathBuf>,
    message: Option<String>,
) -> Result<(), CliError> {
    let bytes = read_input(input)?;
    if let (Some(schema), Some(message)) = (schema, message) {
        let descriptor = load_descriptor(&schema)?;
        let text = tpt20_text::TextFormat::new(&descriptor)
            .print_bytes(&message, &bytes)
            .map_err(text_err)?;
        return write_output_str(&text, output);
    }
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
    let obj = value
        .as_object()
        .ok_or_else(|| CliError::Parse("expected json object".into()))?;

    let mut raw = tpt20_core::RawMessage::new();
    for (key, val) in obj {
        let id: u32 = key
            .parse()
            .map_err(|_| CliError::Parse(format!("invalid field id: {key}")))?;
        let (wire, value) = json_value_to_core(val)?;
        raw.push(tpt20_core::Field::new(id, wire, value));
    }
    let bytes = raw.encode().map_err(|e| CliError::Parse(e.to_string()))?;
    write_output(&bytes, output)
}

fn cmd_text_to_binary(
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    schema: PathBuf,
    message: String,
) -> Result<(), CliError> {
    let text = read_input_string(input)?;
    let descriptor = load_descriptor(&schema)?;
    let bytes = tpt20_text::TextFormat::new(&descriptor)
        .parse_to_bytes(&message, &text)
        .map_err(text_err)?;
    write_output(&bytes, output)
}

fn cmd_binary_to_text(
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    schema: PathBuf,
    message: String,
) -> Result<(), CliError> {
    let bytes = read_input(input)?;
    let descriptor = load_descriptor(&schema)?;
    let text = tpt20_text::TextFormat::new(&descriptor)
        .print_bytes(&message, &bytes)
        .map_err(text_err)?;
    write_output_str(&text, output)
}

fn cmd_json_to_binary(input: Option<PathBuf>, output: Option<PathBuf>) -> Result<(), CliError> {
    let json_str = read_input_string(input)?;
    let value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| CliError::Parse(format!("invalid json: {e}")))?;
    let obj = value
        .as_object()
        .ok_or_else(|| CliError::Parse("expected json object".into()))?;

    let mut raw = tpt20_core::RawMessage::new();
    for (key, val) in obj {
        let id: u32 = key
            .parse()
            .map_err(|_| CliError::Parse(format!("invalid field id: {key}")))?;
        let (wire, value) = json_value_to_core(val)?;
        raw.push(tpt20_core::Field::new(id, wire, value));
    }
    let bytes = raw.encode().map_err(|e| CliError::Parse(e.to_string()))?;
    write_output(&bytes, output)
}

fn cmd_binary_to_json(input: Option<PathBuf>, output: Option<PathBuf>) -> Result<(), CliError> {
    cmd_decode(input, output, None, None)
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

fn json_value_to_core(
    value: &serde_json::Value,
) -> Result<(tpt20_core::WireClass, tpt20_core::Value), CliError> {
    match value {
        serde_json::Value::Null => Ok((
            tpt20_core::WireClass::Len,
            tpt20_core::Value::Len(Vec::new()),
        )),
        serde_json::Value::Bool(b) => Ok((
            tpt20_core::WireClass::Varint,
            tpt20_core::Value::Varint(if *b { 1 } else { 0 }),
        )),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok((
                    tpt20_core::WireClass::Varint,
                    tpt20_core::Value::Varint(i as u64),
                ))
            } else if let Some(u) = n.as_u64() {
                Ok((tpt20_core::WireClass::Varint, tpt20_core::Value::Varint(u)))
            } else if let Some(f) = n.as_f64() {
                Ok((
                    tpt20_core::WireClass::Fixed64,
                    tpt20_core::Value::Fixed64(f.to_bits()),
                ))
            } else {
                Err(CliError::Parse("unsupported number".into()))
            }
        }
        serde_json::Value::String(s) => {
            if let Ok(bytes) = STANDARD.decode(s) {
                Ok((tpt20_core::WireClass::Len, tpt20_core::Value::Len(bytes)))
            } else {
                Ok((
                    tpt20_core::WireClass::Len,
                    tpt20_core::Value::Len(s.as_bytes().to_vec()),
                ))
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
// call (RPC debugger) and health
// ---------------------------------------------------------------------------

struct CallOpts {
    endpoint: String,
    method: String,
    input: Option<PathBuf>,
    binary_input: Option<PathBuf>,
    text_input: Option<PathBuf>,
    schema: Option<PathBuf>,
    request_type: Option<String>,
    response_type: Option<String>,
    streaming: StreamingTypeArg,
    metadata: Vec<String>,
    deadline_ms: Option<u64>,
    tls_cert: Option<PathBuf>,
    compression: CompressionArg,
}

/// Encodes a field-id-keyed JSON object to wire bytes.
fn json_object_to_bytes(value: &serde_json::Value) -> Result<Vec<u8>, CliError> {
    let obj = value
        .as_object()
        .ok_or_else(|| CliError::Parse("expected json object".into()))?;
    let mut raw = tpt20_core::RawMessage::new();
    for (key, val) in obj {
        let id: u32 = key
            .parse()
            .map_err(|_| CliError::Parse(format!("invalid field id: {key}")))?;
        let (wire, value) = json_value_to_core(val)?;
        raw.push(tpt20_core::Field::new(id, wire, value));
    }
    raw.encode().map_err(|e| CliError::Parse(e.to_string()))
}

fn build_endpoint(
    endpoint: &str,
    tls_cert: Option<&Path>,
) -> Result<tpt20_transport::Endpoint, CliError> {
    let (https, address) = if let Some(rest) = endpoint.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = endpoint.strip_prefix("http://") {
        (false, rest)
    } else {
        (false, endpoint)
    };
    let address = address.trim_end_matches('/');
    if address.is_empty() || !address.contains(':') {
        return Err(CliError::Usage(format!(
            "endpoint `{endpoint}` must look like host:port or http(s)://host:port"
        )));
    }
    let mut ep = tpt20_transport::Endpoint::new(address);
    if https || tls_cert.is_some() {
        let cert = tls_cert.ok_or_else(|| {
            CliError::Usage("https endpoints need --tls-cert (PEM CA certificate to trust)".into())
        })?;
        let mut tls = tpt20_transport::TlsConfig::http2();
        tls.cert_pem = Some(fs::read(cert)?);
        ep = ep.with_tls(tls);
    }
    Ok(ep)
}

enum CallOutput {
    Message(Vec<u8>),
    Trailers(Vec<(String, String)>),
}

/// Performs one call and collects everything the server sends back.
async fn perform_call(
    endpoint: tpt20_transport::Endpoint,
    method: &str,
    messages: Vec<Vec<u8>>,
    metadata: &[String],
    streaming: StreamingTypeArg,
    deadline_ms: Option<u64>,
) -> Result<Vec<CallOutput>, CliError> {
    use futures::{SinkExt, StreamExt};
    use tpt20_transport::{StreamItem, StreamingType, Transport};

    let streaming_type = match streaming {
        StreamingTypeArg::Unary => StreamingType::Unary,
        StreamingTypeArg::Server => StreamingType::ServerStream,
        StreamingTypeArg::Client => StreamingType::ClientStream,
        StreamingTypeArg::Bidi => StreamingType::Bidi,
    };
    let client_streams = matches!(
        streaming_type,
        StreamingType::ClientStream | StreamingType::Bidi
    );
    if !client_streams && messages.len() != 1 {
        return Err(CliError::Usage(
            "unary and server-streaming calls take exactly one request message".into(),
        ));
    }

    let mut md = tpt20_transport::Metadata::new();
    for kv in metadata {
        let (k, v) = kv
            .split_once('=')
            .ok_or_else(|| CliError::Usage(format!("metadata `{kv}` must be key=value")))?;
        md.insert(k, v);
    }

    let transport = tpt20_transport::http2::Http2Transport::new(endpoint);
    let run = async {
        let mut messages = messages.into_iter();
        let first = messages.next().unwrap_or_default();
        let mut call = transport
            .start_call(method, first, &md, streaming_type)
            .await
            .map_err(|e| CliError::Transport(e.to_string()))?;
        if client_streams {
            for m in messages {
                call.sink
                    .send(m)
                    .await
                    .map_err(|e| CliError::Transport(e.to_string()))?;
            }
            call.sink
                .close()
                .await
                .map_err(|e| CliError::Transport(e.to_string()))?;
        }
        let mut out = Vec::new();
        while let Some(item) = call.stream.next().await {
            match item.map_err(|e| CliError::Transport(e.to_string()))? {
                StreamItem::Message(bytes) => out.push(CallOutput::Message(bytes)),
                StreamItem::Trailer(t) => {
                    let mut kv: Vec<(String, String)> = t
                        .iter()
                        .flat_map(|(k, vs)| vs.iter().map(move |v| (k.to_string(), v.clone())))
                        .collect();
                    kv.sort();
                    out.push(CallOutput::Trailers(kv));
                }
            }
        }
        Ok::<_, CliError>(out)
    };
    match deadline_ms {
        Some(ms) => tokio::time::timeout(std::time::Duration::from_millis(ms), run)
            .await
            .map_err(|_| CliError::Transport(format!("deadline of {ms}ms exceeded")))?,
        None => run.await,
    }
}

async fn cmd_call(opts: CallOpts) -> Result<(), CliError> {
    if !matches!(opts.compression, CompressionArg::None) {
        return Err(CliError::Usage(
            "compression is not supported by the HTTP/2 transport yet; use --compression none"
                .into(),
        ));
    }
    let descriptor = match &opts.schema {
        Some(p) => Some(load_descriptor(p)?),
        None => None,
    };

    let messages: Vec<Vec<u8>> = if let Some(p) = &opts.binary_input {
        vec![fs::read(p)?]
    } else if let Some(p) = &opts.text_input {
        let (Some(d), Some(ty)) = (&descriptor, &opts.request_type) else {
            return Err(CliError::Usage(
                "--text-input needs --schema and --request-type".into(),
            ));
        };
        let text = fs::read_to_string(p)?;
        vec![tpt20_text::TextFormat::new(d)
            .parse_to_bytes(ty, &text)
            .map_err(text_err)?]
    } else {
        let json = read_input_string(opts.input.clone())?;
        let value: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| CliError::Parse(format!("invalid json: {e}")))?;
        match &value {
            serde_json::Value::Array(items) => items
                .iter()
                .map(json_object_to_bytes)
                .collect::<Result<_, _>>()?,
            other => vec![json_object_to_bytes(other)?],
        }
    };

    let endpoint = build_endpoint(&opts.endpoint, opts.tls_cert.as_deref())?;
    let outputs = perform_call(
        endpoint,
        &opts.method,
        messages,
        &opts.metadata,
        opts.streaming,
        opts.deadline_ms,
    )
    .await?;

    let mut stdout = io::stdout().lock();
    let mut n = 0usize;
    for out in outputs {
        match out {
            CallOutput::Message(bytes) => {
                n += 1;
                writeln!(stdout, "# message {n} ({} bytes)", bytes.len())?;
                let text = match (&descriptor, &opts.response_type) {
                    (Some(d), Some(ty)) => tpt20_text::TextFormat::new(d)
                        .print_bytes(ty, &bytes)
                        .map_err(text_err)?,
                    _ => raw_fields_text(&bytes)?,
                };
                write!(stdout, "{text}")?;
            }
            CallOutput::Trailers(kv) => {
                writeln!(stdout, "# trailers")?;
                for (k, v) in kv {
                    writeln!(stdout, "{k}: {v}")?;
                }
            }
        }
    }
    Ok(())
}

/// Schema-free `id: value` listing of a wire message.
fn raw_fields_text(bytes: &[u8]) -> Result<String, CliError> {
    let raw = tpt20_core::RawMessage::decode(
        bytes,
        &tpt20_core::DecoderLimits::default(),
        tpt20_core::UnknownFieldPolicy::Preserve,
    )
    .map_err(|e| CliError::Parse(e.to_string()))?;
    let mut out = String::new();
    for f in &raw.fields {
        out.push_str(&format!(
            "{}: {}\n",
            f.field_id,
            core_value_to_text(&f.value)
        ));
    }
    Ok(out)
}

/// Native health check convention: `tpt20.health.v1.Health/Check` takes
/// `{1: service string}` and answers `{1: status}` where status is
/// 0 UNKNOWN, 1 SERVING, 2 NOT_SERVING, 3 SERVICE_UNKNOWN (the same numbering
/// as the gRPC health protocol).
const HEALTH_METHOD: &str = "tpt20.health.v1.Health/Check";

async fn cmd_health(
    endpoint: String,
    service: String,
    deadline_ms: u64,
    tls_cert: Option<PathBuf>,
) -> Result<(), CliError> {
    let mut request = tpt20_core::RawMessage::new();
    if !service.is_empty() {
        request.push(tpt20_core::Field::new(
            1,
            tpt20_core::WireClass::Len,
            tpt20_core::Value::Len(service.clone().into_bytes()),
        ));
    }
    let request = request
        .encode()
        .map_err(|e| CliError::Parse(e.to_string()))?;
    let outputs = perform_call(
        build_endpoint(&endpoint, tls_cert.as_deref())?,
        HEALTH_METHOD,
        vec![request],
        &[],
        StreamingTypeArg::Unary,
        Some(deadline_ms),
    )
    .await?;

    let response = outputs
        .into_iter()
        .find_map(|o| match o {
            CallOutput::Message(b) => Some(b),
            CallOutput::Trailers(_) => None,
        })
        .ok_or_else(|| CliError::Transport("server sent no health response".into()))?;
    let raw = tpt20_core::RawMessage::decode(
        &response,
        &tpt20_core::DecoderLimits::default(),
        tpt20_core::UnknownFieldPolicy::Preserve,
    )
    .map_err(|e| CliError::Parse(e.to_string()))?;
    let status = raw
        .fields
        .iter()
        .find(|f| f.field_id == 1)
        .and_then(|f| match f.value {
            tpt20_core::Value::Varint(v) => Some(v),
            _ => None,
        })
        .unwrap_or(0);
    let name = match status {
        1 => "SERVING",
        2 => "NOT_SERVING",
        3 => "SERVICE_UNKNOWN",
        _ => "UNKNOWN",
    };
    let target = if service.is_empty() {
        endpoint.clone()
    } else {
        format!("{endpoint} ({service})")
    };
    println!("{target}: {name}");
    if status == 1 {
        Ok(())
    } else {
        Err(CliError::Transport(format!("service is {name}")))
    }
}

// ---------------------------------------------------------------------------
// reflect
// ---------------------------------------------------------------------------

fn cmd_reflect(file: PathBuf, message: Option<String>) -> Result<(), CliError> {
    let src = fs::read_to_string(&file)?;
    let compiled = tpt20_compiler::compile(&src, file.to_str())
        .map_err(|diags| CliError::Diagnostics(tpt20_compiler::render_all(&diags)))?;

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
                println!(
                    "  enum {}: {}",
                    e.name,
                    if e.open { "open" } else { "closed" }
                );
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

fn registry_root(registry: Option<PathBuf>) -> PathBuf {
    registry.unwrap_or_else(|| {
        home::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".tpt20")
            .join("registry")
    })
}

/// Version labels become directory names; keep them to a safe charset so a
/// label can never escape the registry root.
fn validate_version_label(v: &str) -> Result<(), CliError> {
    let ok = !v.is_empty()
        && v != "."
        && v != ".."
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    if ok {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "invalid version label `{v}` (allowed: letters, digits, `.`, `_`, `-`, `+`)"
        )))
    }
}

fn cmd_registry(command: RegistryCommands) -> Result<(), CliError> {
    match command {
        RegistryCommands::Publish {
            file,
            registry,
            version,
            force,
        } => {
            let registry = registry_root(registry);
            let src = fs::read_to_string(&file)?;
            let compiled = tpt20_compiler::compile(&src, file.to_str())
                .map_err(|diags| CliError::Diagnostics(tpt20_compiler::render_all(&diags)))?;

            let version = version.unwrap_or_else(|| {
                compiled
                    .ir
                    .name
                    .clone()
                    .unwrap_or_else(|| "default".to_string())
            });
            validate_version_label(&version)?;

            let mut manifest = LocalManifest::load_or_default(&registry);
            if let Some(existing) = manifest.versions.iter().find(|v| v.version == version) {
                if existing.fingerprint == compiled.fingerprint {
                    println!(
                        "{version} is already published with this fingerprint ({})",
                        registry.display()
                    );
                    return Ok(());
                }
                if !force {
                    return Err(CliError::Registry(format!(
                        "version `{version}` is already published with a different fingerprint \
                         ({}); published versions are immutable — choose a new --version or pass --force",
                        existing.fingerprint
                    )));
                }
            }

            fs::create_dir_all(&registry)?;
            let version_dir = registry.join(&version);
            fs::create_dir_all(&version_dir)?;
            fs::write(
                version_dir.join("descriptor.json"),
                compiled.descriptor.to_json()?,
            )?;

            manifest.record_version(&version, &compiled.fingerprint, "strict");
            manifest.save(&registry)?;

            println!("published {} to registry ({})", version, registry.display());
            Ok(())
        }
        RegistryCommands::List { registry } => {
            let registry = registry_root(registry);
            let manifest = LocalManifest::load_or_default(&registry);
            if manifest.versions.is_empty() {
                println!("no versions published in {}", registry.display());
                return Ok(());
            }
            println!(
                "{:<24} {:<18} {:<8} PUBLISHED",
                "VERSION", "FINGERPRINT", "POLICY"
            );
            for v in &manifest.versions {
                let fp: String = v.fingerprint.chars().take(16).collect();
                println!(
                    "{:<24} {:<18} {:<8} {}",
                    v.version, fp, v.policy, v.published_at
                );
            }
            Ok(())
        }
        RegistryCommands::Get {
            version,
            registry,
            format,
            out,
        } => {
            let registry = registry_root(registry);
            let manifest = LocalManifest::load_or_default(&registry);
            let record = manifest.find(&version)?;
            validate_version_label(&record.version)?;
            let path = registry.join(&record.version).join("descriptor.json");
            let json = fs::read_to_string(&path)
                .map_err(|e| CliError::Registry(format!("cannot read {}: {e}", path.display())))?;
            let mut descriptor = tpt20_descriptor::Descriptor::from_json(&json)?;
            // Verify integrity: the stored descriptor must still hash to the
            // fingerprint recorded at publish time.
            let actual = descriptor.compute_fingerprint();
            if actual != record.fingerprint {
                return Err(CliError::Registry(format!(
                    "descriptor for `{}` does not match its recorded fingerprint \
                     (recorded {}, actual {actual}); the registry entry was modified",
                    record.version, record.fingerprint
                )));
            }
            match format {
                DescriptorFormat::Json => write_output_str(&descriptor.to_json()?, out),
                DescriptorFormat::Binary => write_output(&descriptor.to_binary()?, out),
            }
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
        LocalManifest {
            versions: Vec::new(),
        }
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
        let json =
            serde_json::to_string_pretty(self).map_err(|e| CliError::Registry(e.to_string()))?;
        fs::write(path, json)?;
        Ok(())
    }

    fn record_version(&mut self, version: &str, fingerprint: &str, policy: &str) {
        // A forced re-publish replaces the earlier record of that label.
        self.versions.retain(|v| v.version != version);
        self.versions.push(VersionRecord {
            version: version.to_string(),
            fingerprint: fingerprint.to_string(),
            policy: policy.to_string(),
            published_at: utc_now_iso8601(),
        });
    }

    /// Finds a record by exact version label, else by fingerprint prefix
    /// (at least 8 characters, and unambiguous).
    fn find(&self, query: &str) -> Result<&VersionRecord, CliError> {
        if let Some(v) = self.versions.iter().find(|v| v.version == query) {
            return Ok(v);
        }
        if query.len() >= 8 {
            let matches: Vec<&VersionRecord> = self
                .versions
                .iter()
                .filter(|v| v.fingerprint.starts_with(query))
                .collect();
            match matches.as_slice() {
                [one] => return Ok(one),
                [] => {}
                _ => {
                    return Err(CliError::Registry(format!(
                        "fingerprint prefix `{query}` is ambiguous"
                    )))
                }
            }
        }
        Err(CliError::Registry(format!(
            "no published version or fingerprint matches `{query}`"
        )))
    }
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ` (no date-time dependency).
fn utc_now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_utc(secs)
}

fn format_unix_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_time_formats_as_utc() {
        assert_eq!(format_unix_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_unix_utc(1_709_210_096), "2024-02-29T12:34:56Z");
        assert_eq!(format_unix_utc(4_102_444_799), "2099-12-31T23:59:59Z");
    }

    #[test]
    fn version_labels_cannot_escape_the_registry() {
        for bad in ["", ".", "..", "../x", "a/b", "a\\b", "sp ace"] {
            assert!(validate_version_label(bad).is_err(), "{bad:?}");
        }
        for ok in ["user.v1", "v1.2.3-rc+build_4"] {
            assert!(validate_version_label(ok).is_ok(), "{ok:?}");
        }
    }

    #[test]
    fn format_preserves_semantics() {
        let src = "package user.v1;\n\nmessage User { 1: id int64; 2: name string; }\n";
        let formatted = format_schema(src);
        let reparsed = tpt20_compiler::compile(&formatted, None);
        assert!(reparsed.is_ok(), "reparsed: {:?}", reparsed);
    }
}

# CLI reference

The `tpt20` binary (`tools/tpt20-cli`) is the developer-facing entry point
for the whole toolchain (spec §21). This reference documents every
subcommand as currently implemented, including where a command is a partial
stub — check here before assuming a flag does something it doesn't yet.

Run `tpt20 <command> --help` for the authoritative, always-in-sync flag list;
this document adds the semantics and caveats `--help` doesn't show.

## `init`

```sh
tpt20 init [--name NAME]
```

Creates a new project directory (`NAME`, defaulting to the current directory
name) containing a starter `.tpt` schema in `src/`, a `Cargo.toml`
referencing `tpt20-core`/`tpt20-runtime`, a `README.md`, and a `.gitignore`.
Fails if the target directory already exists.

## `check`

```sh
tpt20 check <file> [--descriptor]
```

Runs the lexer, parser, and semantic analysis pass and prints diagnostics
(file, line, column, span, severity, error code, explanation, suggested fix
— see [Schema language § Diagnostics](schema-language.md#diagnostics)).
Exits non-zero if any diagnostic has `Error` severity. `--descriptor` also
compiles the schema fully and prints the descriptor as JSON.

## `fmt`

```sh
tpt20 fmt <file> [--check]
```

Rewrites the schema in canonical formatting (consistent indentation and
brace placement), in place. `--check` instead compares the current file
against its formatted form and exits non-zero without writing if they
differ — the pattern used for a CI formatting gate.

## `lint`

```sh
tpt20 lint <files...> [--config FILE] [--format text|json] [--deny-warnings]
```

Runs `check`'s diagnostics plus a configurable set of lint rules. Rules are
listed by name in a TOML config file (default `.tpt20-lint.toml`, key
`rules = ["no-required", ...]`); if no config file exists, all built-in
rules run. Built-in rules:

| Config name | Code | Checks |
|---|---|---|
| `no-required` | `LINT001` (warning) | Source text contains the deprecated `required` keyword. |
| `package-required` | `LINT002` (warning) | Schema has no `package` declaration. |
| `reserved-reuse` | `LINT003` (error) | A `reserved N to M` range has `N >= M`. |
| `deprecated-usage` | `LINT004` (warning) | Source text contains `@deprecated`. |

`--deny-warnings` promotes warnings to failures for CI use.

> These rules currently work by regex/substring matching on the raw source
> text rather than walking the parsed AST, so e.g. `no-required` also flags
> `required` appearing in a comment or string literal.

## `diff`

```sh
tpt20 diff <old> <new>
```

Compiles both schemas and reports every detected change classified `SAFE`,
`WARNING`, or `BREAKING` (see
[Schema language § Schema evolution](schema-language.md#schema-evolution)).
Prints `no differences` if nothing changed.

## `gen rust`

```sh
tpt20 gen rust --in <schema.tpt> --out <dir> [--builders]
```

Compiles the schema and writes `<dir>/<package_file_stem>.rs` (e.g. package
`user.v1` → `user_v1.rs`) using `tpt20-codegen-rust`. See
[Code generation](code-generation.md) for what the output contains.
`--builders` enables generated builder types. Other codegen backends
(`compiler/tpt20-codegen-backends/`) are not yet exposed through `gen`.

## `descriptors`

```sh
tpt20 descriptors <file> [--format json|binary] [--out FILE]
```

Compiles the schema and emits its descriptor — the runtime-usable
representation described in spec §8 — as JSON (default) or the binary
descriptor format, to a file or stdout.

## `decode`

```sh
tpt20 decode [--input FILE] [--output FILE]
```

Decodes native-wire-format bytes (stdin by default) into a JSON object keyed
by **field ID** (not field name) and prints it. **This command has no schema
input** — it operates purely on the raw `(field_id, wire_class, value)`
model, so it cannot tell you a field's name or declared type. Use `tpt20
reflect` for schema-aware inspection. `binary-to-json` is an alias for this
command.

## `encode`

```sh
tpt20 encode [--input FILE] [--output FILE]
```

The inverse of `decode`: reads a JSON object keyed by field ID (values
typed by JSON type — numbers become varints or fixed64 doubles, strings
become length-delimited bytes with best-effort base64 detection, arrays and
objects are serialized as embedded JSON bytes) and writes native wire-format
bytes. `json-to-binary` is an alias for this command.

## `json-to-binary` / `binary-to-json`

Aliases for `encode` and `decode` respectively — same schema-free, field-ID-keyed
behavior described above.

## `text-to-binary` / `binary-to-text`

```sh
tpt20 text-to-binary [--input FILE] [--output FILE]
tpt20 binary-to-text [--input FILE] [--output FILE]
```

> **Not the text format described in spec §14.3.** These commands implement
> an ad hoc `field_id: value` line format (one field per line, values
> inferred as quoted strings, booleans, integers, or floats) rather than a
> real grammar over field *names*, nested messages, repeated fields, maps, or
> oneofs. The proper text format's parser doesn't exist yet (`todo.md` Phase
> 8) — these commands are a stopgap for quick manual inspection of scalar
> fields only.

## `import-proto`

```sh
tpt20 import-proto <input.proto> [--out FILE]
```

Lexes, parses, and lowers a `.proto` file, then prints/writes the resulting
IR **as JSON** — not as regenerated `.tpt` source text. See
[Compatibility adapters § Protobuf schema import](compatibility-adapters.md#protobuf-schema-import)
for supported/unsupported proto features.

## `conformance`

```sh
tpt20 conformance [--directory DIR] [--test NAME]
```

> **Does not run the real conformance suite.** This walks a directory
> (default `tests/conformance`) of `.json` files, and for each one that has a
> top-level `"binary"` string field, hex-decodes it and attempts to
> wire-decode it with default limits — that's the entire test. It does not
> invoke `tools/tpt20-conformance` (the actual native/compatibility
> conformance suite described in spec §22). Use `cargo test -p
> tpt20-conformance` directly for real conformance coverage (subject to the
> compile-status caveats in `todo.md` Phase 17).

## `call`

```sh
tpt20 call <endpoint> <method> [--input FILE] [--binary-input FILE]
           [--streaming unary|server|client|bidi]
           [--metadata key=value ...] [--deadline-ms N]
           [--tls-cert FILE] [--compression none|gzip|deflate]
```

> **Does not perform a network call.** The command parses JSON or binary
> input, builds `Metadata` from `--metadata` pairs, and validates the
> streaming/deadline arguments — but never dials `endpoint` or sends
> anything. `--tls-cert` and `--compression` are accepted and silently
> ignored. Treat this as argument-parsing scaffolding for a future real RPC
> debugger, not a working `grpcurl`-equivalent yet.

## `health`

```sh
tpt20 health <endpoint> [--tls-cert FILE]
```

Prints a placeholder message; does not contact `endpoint`. Not yet wired to
[`tpt20-compat-grpc`'s health protocol support](compatibility-adapters.md#health-checking)
or a native health check.

## `reflect`

```sh
tpt20 reflect <file> [--message NAME]
```

Compiles the schema and, given `--message`, prints the named message's field
list: name, ID, type (including `repeated T` / `map<K, V>` shape), and
presence (`implicit`/`explicit`). This is the schema-aware inspection tool —
prefer it over `decode` when you need field names and types, not just IDs.

## `registry publish`

```sh
tpt20 registry publish <file> [--registry DIR] [--version LABEL]
```

Compiles the schema and writes its descriptor (`descriptor.json`) into
`<registry>/<version>/` (default registry root: `~/.tpt20/registry`; default
version label: the schema's package name), then records the version,
fingerprint, and a `"strict"` compatibility policy in a local
`manifest.json`-style file alongside it.

> **No corresponding lookup/fetch command exists yet.** `registry publish`
> writes files a human or script can read directly from the registry
> directory, but there is no `tpt20 registry get`/`list` to query them back
> through the CLI (`todo.md` Phase 16).

## Exit codes

- `2` — usage error (e.g. a target directory already exists for `init`)
- `1` — any other failure (diagnostics, I/O, parse, registry, transport, JSON errors)
- `0` — success

## Command-by-command implementation status

| Command | Status |
|---|---|
| `init`, `check`, `fmt`, `lint`, `diff` | Fully functional |
| `gen rust` | Fully functional (message/enum codegen only — no service codegen, see [Code generation](code-generation.md)) |
| `descriptors`, `reflect` | Fully functional |
| `decode` / `encode` / `json-to-binary` / `binary-to-json` | Functional, but schema-free (field-ID-keyed) |
| `text-to-binary` / `binary-to-text` | Ad hoc scalar-only format, not the real text format |
| `import-proto` | Functional; emits IR JSON, not `.tpt` source |
| `conformance` | Stub — does not run the real suite |
| `call` | Parses arguments only — makes no network call |
| `health` | Placeholder only |
| `registry publish` | Functional (local filesystem only, no lookup/fetch) |

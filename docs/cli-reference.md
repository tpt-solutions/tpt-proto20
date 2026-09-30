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
tpt20 decode [--input FILE] [--output FILE] [--schema FILE --message NAME]
```

Without a schema, decodes native-wire-format bytes (stdin by default) into a
JSON object keyed by **field ID**, operating purely on the raw
`(field_id, wire_class, value)` model. With `--schema` and `--message`, the
bytes are decoded against that message type and printed in the schema-aware
[text format](#text-to-binary--binary-to-text) (field names, enum names,
nested messages, maps, oneofs). `binary-to-json` is an alias for the
schema-free form.

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
tpt20 text-to-binary --schema FILE --message NAME [--input FILE] [--output FILE]
tpt20 binary-to-text --schema FILE --message NAME [--input FILE] [--output FILE]
```

Convert between the spec §14.3 text format and native wire bytes, driven by
the schema (`tpt20-text`):

```text
id: 42
name: "Ada"
tags: "a"
tags: "b"
home {
  street: "1 Way"
}
attrs {
  key: "k"
  value: "v"
}
```

Fields are named by schema field name; repeated fields are one line per
element (or `tags: ["a", "b"]`); maps are `name { key: … value: … }` entries;
oneofs print only the member that wins on the wire; output order is
deterministic (field id order, map entries sorted by key). `#` starts a
comment. Unknown fields, type mismatches, out-of-range numbers, and
syntax errors are reported as errors (exit 1) with line/column where known.

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

Runs the language-neutral JSON vectors in `DIR` (default
`conformance/vectors`; see [`conformance/README.md`](../conformance/README.md))
and prints `PASS`/`FAIL suite/case` per case plus a summary. The vectors cover
wire decoding (scalars, ordering, malformed input, limits, unknown-field
policies), canonical encoding, round trips, the text format, and schema
diagnostics. `--test` selects a file, suite or single case by name. Exit codes:
`0` all passed, `1` at least one failed, `2` no vectors / nothing matched.
The larger Rust suite runs with `cargo test -p tpt20-conformance`.

## `call`

```sh
tpt20 call <endpoint> <method> [--input FILE | --binary-input FILE |
           --text-input FILE --schema FILE --request-type NAME]
           [--response-type NAME] [--streaming unary|server|client|bidi]
           [--metadata key=value ...] [--deadline-ms N] [--tls-cert FILE]
```

Performs a real call over the HTTP/2 transport and prints every response
message followed by the trailing metadata:

```text
# message 1 (9 bytes)
1: 42
# trailers
x-status: ok
```

- `endpoint` is `host:port` or `http(s)://host:port`. `https://` (or
  `--tls-cert`) enables TLS with ALPN `h2`; `--tls-client-cert FILE --tls-client-key FILE` (global options, before or
  after the subcommand) present a client certificate to servers that require
  mTLS; `--tls-cert` is the PEM CA
  certificate to trust.
- Request input: `--input` is a JSON object keyed by field ID (an array of
  objects sends several messages for `client`/`bidi` streams);
  `--binary-input` is one raw wire message; `--text-input` parses the text
  format against `--schema`/`--request-type`. With no input flag the JSON is
  read from stdin.
- Responses print in text format when `--schema` and `--response-type` are
  given, otherwise as a schema-free `id: value` listing.
- `--deadline-ms` is enforced client-side (exit 1 when exceeded).
- `--compression gzip|deflate` compresses request messages (each one that gets
  smaller); the server answers compressed if it is configured to and the
  client advertised support, which `call` always does. Any other algorithm is
  a usage error (exit 2).
- Unary and server-streaming calls require exactly one request message.

## `health`

```sh
tpt20 health <endpoint> [--service NAME] [--deadline-ms N] [--tls-cert FILE]
```

Calls `tpt20.health.v1.Health/Check` (served by
`tpt20_rpc::health::HealthService`, see [RPC model](rpc-model.md#built-in-health-and-reflection-services))
with the service name (empty for the overall server) and prints
`<endpoint>: <STATUS>` (`UNKNOWN`, `SERVING`, `NOT_SERVING`,
`SERVICE_UNKNOWN`). Exits `0` only for `SERVING`, otherwise `1`.

## `reflect-remote`

```sh
tpt20 reflect-remote <endpoint> [--descriptor] [--package NAME]
                     [--format json|binary] [--out FILE] [--deadline-ms N] [--tls-cert FILE]
```

Asks a running server (one that registered
`tpt20_rpc::reflection::ReflectionService`) what it serves. Without
`--descriptor` it prints the fully qualified service names, one per line; with
it, the schema descriptor of `--package` (default: the only registered one) as
JSON or binary. Use it to get a schema for `decode --schema`-style tooling
without the original `.tpt` files.

## `reflect`

```sh
tpt20 reflect <file> [--message NAME]
```

Compiles the schema and, given `--message`, prints the named message's field
list: name, ID, type (including `repeated T` / `map<K, V>` shape), and
presence (`implicit`/`explicit`). This is the schema-aware inspection tool —
prefer it over `decode` when you need field names and types, not just IDs.

## `registry`

```sh
tpt20 registry publish <file> [--registry DIR] [--version LABEL] [--force]
tpt20 registry list [--registry DIR]
tpt20 registry get <version|fingerprint-prefix> [--registry DIR]
                   [--format json|binary] [--out FILE]
```

A local-filesystem registry (default root `~/.tpt20/registry`):

- `publish` compiles the schema and writes its descriptor
  (`<registry>/<version>/descriptor.json`) plus a manifest entry (version,
  fingerprint, `"strict"` policy, UTC publish time). The version label
  defaults to the package name and may only contain letters, digits and
  `. _ - +`. **Published versions are immutable:** re-publishing the same
  content is a no-op; different content under an existing label is an error
  unless `--force` is given.
- `list` prints versions with (truncated) fingerprints, policy and publish
  time.
- `get` fetches a descriptor by version label or by a fingerprint prefix
  (≥ 8 characters, unambiguous) and prints it as JSON or writes the binary
  form. It **verifies integrity**: the stored descriptor is re-fingerprinted
  and must match the manifest, otherwise the command fails.

The `"strict"` compatibility policy is recorded but not enforced at publish
time yet (`tpt20 diff` checks two schema files directly).

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
| `decode` / `encode` / `json-to-binary` / `binary-to-json` | Functional; schema-free (field-ID-keyed), except `decode --schema --message` which is schema-aware |
| `text-to-binary` / `binary-to-text` | Fully functional, schema-driven text format |
| `import-proto` | Functional; emits IR JSON, not `.tpt` source |
| `conformance` | Functional: runs the JSON conformance vectors in `conformance/vectors` |
| `call` | Fully functional over HTTP/2 (TLS, metadata, deadline, streaming, compression) |
| `health` | Fully functional (`tpt20.health.v1.Health/Check`) |
| `reflect-remote` | Fully functional (`tpt20.reflection.v1.Reflection`) |
| `registry publish` / `list` / `get` | Functional (local filesystem; immutable versions, integrity-checked fetch) |

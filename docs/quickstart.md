# Quickstart

This walks through writing a `.tpt` schema, checking it, generating Rust
code, and encoding/decoding a message with the CLI — the fastest path to
seeing the whole pipeline work end to end.

## 1. Build the CLI

From the workspace root:

```sh
cargo build -p tpt20-cli --release
```

The examples below assume the binary is on your `PATH` as `tpt20`
(`cargo run -p tpt20-cli --` also works and is used in the snippets).

## 2. Start a project

```sh
cargo run -p tpt20-cli -- init my-app
cd my-app
```

`tpt20 init` creates:

```text
my-app/
  Cargo.toml
  README.md
  .gitignore
  src/my-app.tpt
```

The generated schema is a minimal starting point:

```tpt
package my-app;

message Example {
    1: id int64;
    2: name string;
}
```

Replace it with your own schema — package names conventionally use
`domain.vN` style versioning (see [Schema language reference](schema-language.md)).

## 3. Write a schema

```tpt
package user.v1;

message User {
  1: id int64;
  2: name string;
  3: email string?;
  4: roles repeated Role;
}

enum Role {
  UNKNOWN = 0;
  ADMIN = 1;
  MEMBER = 2;
}
```

Save this as `src/user.v1.tpt`.

## 4. Check the schema

```sh
tpt20 check src/user.v1.tpt
```

This runs the lexer, parser, and semantic analysis pass and prints
diagnostics (file, line, column, error code, and a suggested fix where one
exists) without generating any code. Add `--descriptor` to also print the
compiled descriptor as JSON.

## 5. Generate Rust code

```sh
tpt20 gen rust --in src/user.v1.tpt --out src/generated
```

This writes `src/generated/user_v1.rs`, a self-contained module with:

- an owned struct per message (`User`)
- a borrowed view type (`UserView<'_>`)
- `encode` / `decode` / `decode_with_limits` / `decode_borrowed`
- `to_json` / `from_json`
- a Rust enum for `Role` with `from_i32` / `to_i32`

See [Code generation](code-generation.md) for the full shape of the output
and how to wire the generated module into your crate.

## 6. Encode and decode from the command line

Without writing any Rust, you can round-trip raw field data through the
native wire format directly from JSON, keyed by field ID:

```sh
echo '{"1": 42, "2": "Ada"}' | tpt20 encode > user.bin
tpt20 decode < user.bin
# {"1": 42, "2": "Ada"}
```

`encode`/`decode` (and `json-to-binary`/`binary-to-json`, their aliases) work
on the raw field model and do not require a schema — field numbers are wire
tags, not semantic names. For schema-aware inspection, use `tpt20 reflect`:

```sh
tpt20 reflect src/user.v1.tpt --message user.v1.User
```

## 7. Inspect compatibility between schema versions

```sh
tpt20 diff src/v1/user.tpt src/v2/user.tpt
```

Output classifies every change as `SAFE`, `WARNING`, or `BREAKING` (see
[Schema language reference § Evolution](schema-language.md#schema-evolution)).

## Where to go next

- [Schema language reference](schema-language.md) — the full `.tpt` grammar
- [Code generation](code-generation.md) — what the Rust backend emits
- [CLI reference](cli-reference.md) — every subcommand and its current status
- [Wire format specification](wire-format.md) — what `encode`/`decode` actually do on the wire
- [RPC model](rpc-model.md) — services, streaming, and status codes

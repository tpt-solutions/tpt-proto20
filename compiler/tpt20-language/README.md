# tpt20-language

Lexer, parser, and AST for the `.tpt` schema language (Phase 1, spec §6).

## Overview

This crate implements the front end of the tpt20 compiler pipeline:

```
.tpt source → lexer → parser → AST
```

It provides:

- [`lexer`] — tokenizer producing [`Token`] values with [`Span`] information
- [`parser`] — recursive-descent parser producing an [`ast::File`]
- [`ast`] — public data structures for messages, fields, enums, services, oneofs, maps, annotations, and imports

The AST is consumed by semantic analysis in `tpt20-compiler` and lowered to the neutral IR in `tpt20-ir`.

## Usage

```rust
use tpt20_language::parse_file;

let src = r#"
package user.v1;

message User {
    1: id int64;
    2: name string;
    3: email string?;
}
"#;

let file = parse_file(src).expect("valid schema");
println!("package: {:?}", file.package);
```

## Crate layout

- `src/ast.rs` — AST types
- `src/lexer.rs` — tokenizer
- `src/parser.rs` — parser

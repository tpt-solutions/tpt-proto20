# tpt20-codegen-tests

Compile-and-run tests for `tpt20-codegen-rust` output (Phase 5).

## Overview

`build.rs` compiles the fixture schema and generates a Rust module into `OUT_DIR`; this crate includes it, so every test exercises real generated code:

- Wire roundtrips
- Canonical encoding
- Unknown-field preservation
- Decoder limits
- Borrowed views
- JSON conversion
- Builders

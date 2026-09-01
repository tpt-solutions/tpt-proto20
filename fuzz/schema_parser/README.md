# schema-parser

Fuzz target for the tpt20 schema parser.

## Overview

Standalone fuzz binary targeting `tpt20-language::parser` to discover edge cases in `.tpt` parsing.

## Usage

```sh
cargo fuzz run schema-parser
```

# text-parser

Fuzz target for the tpt20 text parser.

## Overview

Standalone fuzz binary targeting the `.tpt` schema text parser to discover edge cases in lexer/parser handling of malformed input.

## Usage

```sh
cargo fuzz run text-parser
```

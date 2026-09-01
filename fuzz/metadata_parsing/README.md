# metadata-parsing

Fuzz target for tpt20 metadata parsing.

## Overview

Standalone fuzz binary targeting `tpt20-rpc::Metadata` to discover edge cases in metadata parsing and normalization.

## Usage

```sh
cargo fuzz run metadata-parsing
```

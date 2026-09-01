# json-decoder

Fuzz target for the tpt20 JSON decoder.

## Overview

Standalone fuzz binary targeting `tpt20-json` to discover edge cases in JSON-to-message conversion.

## Usage

```sh
cargo fuzz run json-decoder
```

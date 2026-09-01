# binary-decoder

Fuzz target for the tpt20 binary decoder.

## Overview

Standalone fuzz binary targeting `tpt20-core::RawMessage::decode` to discover edge cases in the native wire format decoder.

## Usage

```sh
cargo fuzz run binary-decoder
```

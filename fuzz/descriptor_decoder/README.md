# descriptor-decoder

Fuzz target for the tpt20 descriptor decoder.

## Overview

Standalone fuzz binary targeting `tpt20-descriptor::from_binary` to discover edge cases in descriptor deserialization.

## Usage

```sh
cargo fuzz run descriptor-decoder
```

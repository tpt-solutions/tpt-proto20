# dynamic-message-decoder

Fuzz target for the tpt20 dynamic message decoder.

## Overview

Standalone fuzz binary targeting `tpt20-reflect::DynamicMessage::decode` to discover edge cases in descriptor-driven message decoding.

## Usage

```sh
cargo fuzz run dynamic-message-decoder
```

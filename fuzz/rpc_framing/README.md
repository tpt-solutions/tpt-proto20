# rpc-framing

Fuzz target for tpt20 RPC framing.

## Overview

Standalone fuzz binary targeting `tpt20-transport::frame` to discover edge cases in message framing and length-delimited encoding.

## Usage

```sh
cargo fuzz run rpc-framing
```

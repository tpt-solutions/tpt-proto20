# tpt20-conformance-tests

Integration tests for the tpt20 conformance suite.

## Overview

These tests exercise the same APIs as the conformance crate modules but as a separate test binary to ensure cross-crate integration works.

Modules:

- `native` — native conformance tests
- `compat` — compatibility conformance tests
- `roundtrip` — property-based roundtrip tests
- `interop` — cross-implementation interop tests

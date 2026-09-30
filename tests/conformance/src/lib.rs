//! Integration tests for the tpt20 conformance suite.
//!
//! These tests exercise the same APIs as the conformance crate modules but
//! as a separate test binary to ensure cross-crate integration works.

pub mod compat;
pub mod interop;
pub mod native;
pub mod roundtrip;

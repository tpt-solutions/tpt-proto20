//! Integration tests for the tpt20 conformance suite.
//!
//! These tests exercise the same APIs as the conformance crate modules but
//! as a separate test binary to ensure cross-crate integration works.

#[cfg(test)]
pub mod compat;
#[cfg(test)]
pub mod interop;
#[cfg(test)]
pub mod native;
#[cfg(test)]
pub mod roundtrip;

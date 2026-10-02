//! Shared fixtures for the benchmarks: generated code for `schema.tpt`.

#[allow(unused, non_camel_case_types, clippy::all)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

/// The schema the benchmarks are generated from.
pub const SCHEMA: &str = include_str!("../schema.tpt");

//! Host tests of the pure half of the COSE binding against the vectors
//! shared with `cratestack-cose`, the web build and the Rust client
//! (`crates/cratestack-cose/tests/vectors`, read in place at compile time,
//! so no vector file is copied). The napi shell around it is driven by the
//! vitest suite of `packages/cratestack-cbor-node`.

mod external;
mod rejects;
mod support;
mod vectors;

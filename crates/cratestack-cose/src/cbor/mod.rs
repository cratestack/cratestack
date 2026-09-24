//! The minimum of CBOR this crate needs, written by hand.
//!
//! Hand-rolled rather than borrowed from `coset`/`ciborium` or
//! `minicbor`, for two reasons ADR 0006 and the P0 scoping pass state:
//! `coset` cannot encode the payload in place or hand back a zero-copy
//! slice of what it parsed, and neither it nor a general-purpose decoder
//! is strict about non-minimal heads, indefinite lengths or duplicate map
//! keys, all of which change the bytes without changing the meaning. The
//! envelope reads and writes about ten item kinds; a reader that accepts
//! exactly those is shorter than a configuration that restricts a general
//! one, and easier to audit. `tests/interop_coset.rs` checks the result
//! against `coset` byte for byte.

pub(crate) mod read;
pub(crate) mod write;

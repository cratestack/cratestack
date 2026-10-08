//! The COSE signed transport for the web (cratestack#1026, ADR 0006 §11):
//! a `ClientEnvelope` class that seals requests and opens responses with
//! `cratestack-cose`, the one implementation. Behind the `cose` feature,
//! which `@cratestack/cbor-web` is built without; only the Dart package's
//! web artifact has it.
//!
//! ```text
//! JS binding object ──▶ CallBinding ──▶ Binding ──▶ CoseEnvelope
//!   ▲                   (canonical query, AAD inputs: cratestack-cose)
//!   └── Promise<Uint8Array> / { payload, kid, alg, thumbprint } / { code, message }
//! ```
//!
//! A call returns a `Promise`: the envelope's methods are async (a signer
//! may be), and the future runs on the browser's microtask queue through
//! `future_to_promise` over a clone of the envelope, which is an `Arc`.
//! Errors are plain `{ code, message }` objects, `code` being `"rejected"`
//! (every failed verification, with an empty message) or `"misuse"`.

mod convert;
mod envelope;
mod error;

#[cfg(test)]
mod tests;

pub use envelope::{ClientEnvelope, contract_header_value};

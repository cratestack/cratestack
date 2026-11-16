//! The generated Rust client opening a `@frames(unsigned)` subscription over
//! the signed transport, `the REST transport` (cratestack#1378). The cases are in
//! `cose_client_stream_support/cases.rs` and `refusal_cases.rs`.

mod cose_client_stream_support;
mod cose_client_support;
#[path = "cose_client_stream_support/rest_app.rs"]
mod rest_app;

client_stream_cases!(rest_app);
client_stream_refusal_cases!(rest_app);

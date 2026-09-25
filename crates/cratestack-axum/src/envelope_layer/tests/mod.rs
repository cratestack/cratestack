//! The envelope layer against hand-built routers, with the COSE envelope
//! and the published test keys of `cratestack-cose/tests/vectors/keys.json`
//! (never use them elsewhere). Generated routers, the rate limiter and the
//! idempotency layer are exercised in `cratestack-api/tests/cose_*.rs`.

mod adversarial;
mod adversarial_envelope;
mod fixtures;
mod mounts;
mod off_mode;
mod optional;
mod plugins;
mod required_rest;
mod required_rest_errors;
mod required_rpc;
mod streams;
mod support;
mod toy;

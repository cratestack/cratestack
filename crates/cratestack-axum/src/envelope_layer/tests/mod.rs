//! The envelope layer against hand-built routers. The toy envelope's tests
//! need only the `envelope` feature (they prove the layer works with no
//! COSE crate at all); the rest use the COSE envelope and the published test
//! keys of `cratestack-cose/tests/vectors/keys.json` (never use them
//! elsewhere). Generated routers, the rate limiter and the idempotency
//! layer are exercised in `cratestack-api/tests/cose_*.rs`.

mod adversarial_envelope;
mod fixtures;
mod hostile;
mod support;
mod toy;

#[cfg(feature = "cose")]
mod adversarial;
#[cfg(feature = "cose")]
mod batch_policy;
#[cfg(feature = "cose")]
mod bound_headers;
#[cfg(feature = "cose")]
mod cose_support;
#[cfg(feature = "cose")]
mod fail_closed;
#[cfg(feature = "cose")]
mod mounts;
#[cfg(feature = "cose")]
mod off_mode;
#[cfg(feature = "cose")]
mod optional;
#[cfg(feature = "cose")]
mod optional_signed;
#[cfg(feature = "cose")]
mod plugins;
#[cfg(feature = "cose")]
mod required_rest;
#[cfg(feature = "cose")]
mod required_rest_errors;
#[cfg(feature = "cose")]
mod required_rpc;
#[cfg(feature = "cose")]
mod rpc_modes;
#[cfg(feature = "cose")]
mod rpc_rules;
#[cfg(feature = "cose")]
mod streams;

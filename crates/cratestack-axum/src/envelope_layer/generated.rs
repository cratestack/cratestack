//! The body of the generated per-schema `envelope_layer(envelope, policy,
//! audience)` (cratestack#1006): an [`super::EnvelopeLayerBuilder`] already
//! set to the schema's transport, its `ROUTE_TRANSPORTS` and its
//! `SCHEMA_SHA256_BYTES`, so a REST schema's layer cannot be built as RPC
//! (or over another schema's descriptors).
//!
//! **Who decides whether it is emitted** (second-review decision S-1,
//! 2026-09-26): not this crate and not `cratestack-macros`, whose features
//! are unified across the whole build. `include_server_schema!` emits
//! `::cratestack::__envelope_layer_fn!(rest)` or `(rpc)`, and each server
//! facade (`cratestack-pg`, `cratestack-api`) defines that macro twice,
//! under its own `envelope` feature: on, it forwards here; off, it emits
//! nothing. So a crate that turns `envelope` on cannot change what another
//! crate's schema expands to through some other facade, as the former
//! graph-wide `cratestack-macros/envelope` feature could. The body lives
//! here, once, rather than in each facade, so the two cannot drift; a
//! facade only forwards here when its `envelope` feature is on, which
//! turns this crate's on too.
//!
//! `ROUTE_TRANSPORTS` and `super::SCHEMA_SHA256_BYTES` resolve where the
//! macro is invoked (the generated `axum` module): `macro_rules!` hygiene
//! is call-site for everything but local variables, labels and `$crate`.

/// Not public API: invoked only through a facade's `__envelope_layer_fn!`.
#[doc(hidden)]
#[macro_export]
macro_rules! __envelope_layer_fn_body {
    (rest) => {
        $crate::__envelope_layer_fn_body!(@emit .rest("", ROUTE_TRANSPORTS));
    };
    (rpc) => {
        $crate::__envelope_layer_fn_body!(@emit .rpc(""));
    };
    (@emit $($transport:tt)*) => {
        /// The envelope layer for this schema's router (ADR 0006,
        /// cratestack#1006): `envelope` opens and seals, `policy` picks
        /// each op's mode, `audience` is this service's configured id. The
        /// transport, the route descriptors and the schema digest are this
        /// schema's; the body cap is the default, which fits the routers'
        /// default body limit (raise it with `max_body_bytes` if the router
        /// gets a larger one). Mounted under a prefix, add
        /// `.mount_prefix("/api")`; then `.build()`, and apply it as the
        /// router's last `.layer(..)`.
        pub fn envelope_layer(
            envelope: impl $crate::envelope_layer::ServerEnvelope,
            policy: impl $crate::envelope_layer::EnvelopePolicy,
            audience: impl ::core::convert::Into<::std::string::String>,
        ) -> $crate::envelope_layer::EnvelopeLayerBuilder {
            $crate::envelope_layer::EnvelopeLayer::builder(
                envelope,
                audience,
                super::SCHEMA_SHA256_BYTES,
            )
            .policy(policy)
            $($transport)*
            .max_body_bytes($crate::envelope_layer::DEFAULT_MAX_BODY_BYTES)
        }
    };
}

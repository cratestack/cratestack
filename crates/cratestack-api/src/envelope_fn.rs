//! `__envelope_layer_fn!`, which `include_server_schema!` invokes as
//! `::cratestack::__envelope_layer_fn!(rest)` or `(rpc)` in every generated
//! `axum` module (cratestack#1006, second-review decision S-1).
//!
//! It is defined here, per facade, so that **this facade's** `envelope`
//! feature decides whether a schema compiled through it gets the generated
//! `envelope_layer(envelope, policy, audience)`: on, the macro forwards to
//! `cratestack_axum`'s body; off, it expands to nothing. It used to be
//! `cratestack-macros/envelope`, a proc-macro feature and so unified across
//! the whole build: one crate turning `envelope` on changed what every
//! other server schema in the build expanded to, and broke those compiled
//! through a facade without the layer. `cratestack-pg` and `cratestack-api`
//! carry the same two definitions; the function body is written once, in
//! `cratestack_axum::envelope_layer`'s `generated` module.

/// Not public API: the target of generated code only.
#[cfg(feature = "envelope")]
#[doc(hidden)]
#[macro_export]
macro_rules! __envelope_layer_fn {
    ($transport:ident) => {
        $crate::__private::envelope_layer_fn_body!($transport);
    };
}

/// Not public API: without this facade's `envelope` feature, a schema gets
/// no `envelope_layer`, whatever the rest of the build enables.
#[cfg(not(feature = "envelope"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __envelope_layer_fn {
    ($transport:ident) => {};
}

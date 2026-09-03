//! L3 — Execution. The transport-neutral middle of an operation.
//!
//! This crate is the `OpExecutor` [`docs/design/rpc-transport.md`][rpc] §4
//! has specified since 2026-05-15 and [`docs/design/layering.md`][lay] §2
//! has named as the one layer that did not exist. ADR 0015 (accepted,
//! amended 2026-09-03) settles building it; this is slice 1, which moves
//! **idempotency admission only** — see "Scope" below.
//!
//! [rpc]: https://github.com/cratestack/cratestack/blob/main/docs/design/rpc-transport.md
//! [lay]: https://github.com/cratestack/cratestack/blob/main/docs/design/layering.md
//!
//! # Why the dependency list is one workspace crate long
//!
//! `layering.md` §2's L3 section states two exclusions, and both of them
//! are load-bearing rather than stylistic:
//!
//! - **Nothing transport-shaped.** No `http::HeaderMap`, no `tower::Layer`,
//!   no `axum::Response`. That is why [`OpInput::fingerprint`] arrives
//!   already computed instead of being hashed here: method, path+query and
//!   content-type are transport facts, and a `[u8; 32]` that the HTTP
//!   adapter still produces byte-for-byte the way it always did is what
//!   makes "the wire did not change" a provable claim rather than a hope.
//! - **Nothing backend-shaped.** No `sqlx::Transaction`, no `rusqlite`.
//!   Audit *persistence* in particular cannot move here — it commits inside
//!   the mutation's own transaction (`cratestack-sqlx/src/audit.rs`), and
//!   threading `&mut Transaction` through a transport-neutral interface is
//!   exactly what this exclusion forbids (ADR 0015, Consequences).
//!
//! Subtract both and what is left needs `cratestack-core` (for
//! [`cratestack_core::IdempotencyStore`], `CratestackError`,
//! `OpDescriptor`/`RouteTransportDescriptor`) and `uuid` (the reservation
//! token type the L1 store trait already uses). There is no third
//! dependency to justify, which is the shape the layer was supposed to
//! have.
//!
//! # ADR 0012: a function over collaborators, never a registry
//!
//! [`OpExecutor`] is constructed with the collaborators it will use and
//! holds them in named fields. It does not look anything up by type at
//! runtime. ADR 0012 (accepted) rejects an IoC container specifically
//! because a type-keyed lookup would make
//! `examples/no-database-verification`'s proof — `cargo tree | grep -i
//! sqlx` returning nothing — unstateable: what a container resolves is not
//! visible in the dependency graph. ADR 0015's decision text repeats the
//! constraint for this crate by name ("a function over an already-chosen
//! set of collaborators, never a registry").
//!
//! # Scope of slice 1
//!
//! Only **idempotency admission** — [`OpExecutor::admit`],
//! [`OpExecutor::complete`], [`OpExecutor::release`]. Rate limiting stays
//! at L4 (`cratestack-axum::ratelimit`), audit stays where it is, row-level
//! policy on subscriptions stays unenforced, and
//! [`OpInput::ctx`] is always `None`. Those are later slices, not
//! omissions; `OpAdmission::rate_limited_by_default` is carried through
//! today so the input shape does not have to change when rate limiting
//! follows.

mod admission;
mod executor;
mod input;

#[cfg(test)]
mod tests_admission;
#[cfg(test)]
mod tests_executor;

pub use admission::Admission;
pub use executor::OpExecutor;
pub use input::{OpAdmission, OpInput};

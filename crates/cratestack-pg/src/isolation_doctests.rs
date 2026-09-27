//! Compile-time half of the `@isolation` escape test (GHSA-r67q-4qqq-g9gm,
//! docs/design/procedure-isolation.md §3): the handle an `@isolation`
//! procedure receives has no way to reach the pool. The runtime half —
//! everything the handle *does* offer runs inside the procedure's
//! transaction — is `tests/procedure_isolation.rs`.
//!
//! Two twins over the same schema. The first compiles: it uses what the
//! handle offers (model accessors, `transaction`). The second differs only
//! by the `db.pool()` line and must not compile (E0599, "no method named
//! `pool`"). Stable rustdoc does not check the error code of a
//! `compile_fail` block — measured: `E0308` there passes too — so it is
//! the compiling twin, identical but for that line, that keeps the
//! failure from being about something else. Keep them in step.
//!
//! ```no_run
//! cratestack::include_server_schema!("tests/fixtures/procedure_isolation.cstack", db = Postgres);
//!
//! async fn uses_the_handle(
//!     db: &cratestack_schema::IsolatedCratestack,
//!     ctx: &cratestack::CratestackContext,
//! ) -> Result<(), cratestack::CratestackError> {
//!     let _ = db.iso_account().find_unique(1).run(ctx).await?;
//!     db.transaction(async |_tx| Ok(())).await
//! }
//!
//! fn main() {
//!     let _ = uses_the_handle;
//! }
//! ```
//!
//! ```compile_fail,E0599
//! cratestack::include_server_schema!("tests/fixtures/procedure_isolation.cstack", db = Postgres);
//!
//! async fn reaches_for_the_pool(
//!     db: &cratestack_schema::IsolatedCratestack,
//!     ctx: &cratestack::CratestackContext,
//! ) -> Result<(), cratestack::CratestackError> {
//!     let _ = db.iso_account().find_unique(1).run(ctx).await?;
//!     let _ = db.pool();
//!     db.transaction(async |_tx| Ok(())).await
//! }
//!
//! fn main() {
//!     let _ = reaches_for_the_pool;
//! }
//! ```

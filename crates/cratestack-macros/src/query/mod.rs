//! `query` block code emission (cratestack#867; accepted design
//! `docs/design/declarative-custom-query.md`).
//!
//! Mirrors `view/`'s module layout deliberately — the design picks `view`
//! as the model precisely because a view is already a server-only,
//! SQL-defined, column-name-decoded construct that no route, op
//! descriptor or client generator ever iterates. The differences from
//! `view/` are the ones the construct actually needs:
//!
//! - No `descriptor` — a `query` has no `ViewDescriptor`/`ModelDescriptor`
//!   equivalent because nothing about the framework's `Filter`/`OrderClause`
//!   AST applies to an opaque SQL body (design §7).
//! - No `row_sqlite` — Postgres-only (design §4).
//! - No `struct_only` — the result shape is a reference to a `type`
//!   declaration that `types::generate_type_struct` already emitted, not a
//!   new struct of its own (design §3).
//! - A [`entry`] module that `view` has no counterpart for: the single
//!   generated `run` function where the `@allow`/`@deny` check happens.
//!
//! Submodules:
//! - [`shim`] — the `Query` → `Procedure` adaptor that lets the existing
//!   model-agnostic policy resolver and `Args` generator be reused.
//! - [`row_pg`] — `impl sqlx::FromRow` for each result `type`.
//! - [`entry`] — the `SQL` const, the bind chain and `run`.
//! - [`module`] — the per-query `pub mod <query_snake>` assembly.
//! - [`accessor`] — the `Cratestack::queries().<query_snake>()` method.

pub(crate) mod accessor;
pub(crate) mod entry;
pub(crate) mod module;
pub(crate) mod row_pg;
pub(crate) mod shim;

pub(crate) use accessor::generate_query_accessor;
pub(crate) use module::generate_query_module;
pub(crate) use row_pg::generate_query_result_from_row_impls;

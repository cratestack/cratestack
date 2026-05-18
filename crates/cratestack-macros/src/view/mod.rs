//! View block code emission (ADR-0003). Mirrors the `model/` submodule
//! but produces the narrower read-only surface views need.
//!
//! - [`struct_only`] — the `pub struct <ViewName>` declaration.
//! - [`descriptor`] — the `pub const <UPPER>_VIEW: ViewDescriptor<...>`
//!   static + the per-view `ReadPolicy` arrays.
//! - [`row_pg`] — `impl sqlx::FromRow` for the server composer.
//! - [`row_sqlite`] — `impl FromRusqliteRow` for the embedded composer.
//! - [`accessor`] — the `runtime.views().<view_snake>()` method body.

pub(crate) mod accessor;
pub(crate) mod descriptor;
pub(crate) mod row_pg;
pub(crate) mod row_sqlite;
pub(crate) mod struct_only;

pub(crate) use accessor::generate_view_accessor;
pub(crate) use descriptor::generate_view_descriptor;
pub(crate) use row_pg::generate_view_pg_from_row_impl;
pub(crate) use row_sqlite::generate_view_rusqlite_from_row_impl;
pub(crate) use struct_only::generate_view_struct_only;

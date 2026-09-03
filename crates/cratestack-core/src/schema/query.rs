//! `query` IR — the declarative, parameterized custom-SQL read block
//! (cratestack#867; accepted design `docs/design/declarative-custom-query.md`,
//! adopted by epic cratestack#488's 2026-09-02 decision comment).
//!
//! A `query` is deliberately *not* a `procedure` with a body and *not* a
//! parameterized `view`:
//!
//! - Unlike [`Procedure`](super::Procedure), it carries a SQL body and is
//!   never public wire surface — no REST route, no RPC op id, no generated
//!   client stub. That is why it is a separate list on
//!   [`Schema`](super::Schema) rather than a new `Procedure` field: every
//!   route/op/client emission site iterates `procedures` and `models`, so a
//!   construct they never loop over costs nothing, whereas a flag on
//!   `Procedure` would need an "is this one internal" branch at five sites
//!   (design §1).
//! - Unlike [`View`](super::View), it emits no DDL and creates no
//!   persistent database object, and its SQL takes runtime bind parameters
//!   — which `CREATE VIEW` has no slot for at all.
//!
//! The arg list reuses [`ProcedureArg`] verbatim rather than defining a
//! near-identical twin: the policy resolver
//! (`cratestack-macros/src/policy/procedure/resolver.rs`) resolves
//! `@allow`/`@deny` predicates purely against an arg list and the schema's
//! `type` declarations, with no model dependency, so sharing the type is
//! what lets a `query` reuse it with no new machinery (design §6).

pub mod placeholders;

use serde::{Deserialize, Serialize};

use super::SourceSpan;
use super::model::{Attribute, TypeRef};
use super::procedure::ProcedureArg;
use super::sql_body::extract_sql_body;

pub use placeholders::scan_sql_placeholders;

/// The attribute a `query` block's SQL body is written in.
///
/// Only `@@sql` — there is deliberately no `@@server_sql`/`@@embedded_sql`
/// split the way [`View`](super::View) has one. A `query` is Postgres-only
/// in v1 (design §4): the escape hatch exists precisely to write Postgres
/// spellings like `FILTER (WHERE …)` and `::bigint` that no portable
/// dialect layer can credibly translate, so a second "embedded" body would
/// be promising something the construct cannot deliver.
pub const QUERY_SQL_ATTRIBUTE: &str = "@@sql";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    /// Declared parameters, bound positionally: the first arg is `$1`.
    /// Shared with [`Procedure`](super::Procedure) — see the module doc.
    pub args: Vec<ProcedureArg>,
    /// The author-declared result shape. Always a `type` declaration's
    /// name (checked by the parser's semantic pass), at `Required` arity
    /// for a single row or `List` arity (`T[]`) for many.
    pub result_type: TypeRef,
    /// Block attributes: the `@@sql(…)` body plus `@allow`/`@deny`.
    /// Stored raw, same as [`Procedure::attributes`](super::Procedure).
    pub attributes: Vec<Attribute>,
    pub span: SourceSpan,
}

impl Query {
    /// The SQL text declared via `@@sql("…")` / `@@sql("""…""")`.
    ///
    /// Never *rewritten* anywhere in the pipeline — the `$N` validator
    /// only scans it, and no code path substitutes a caller-supplied
    /// value into it. That is the property that makes the construct safe
    /// (design §2/§7). The single-line form's `\"`/`\\` unescaping
    /// (see [`extract_sql_body`]) is a *decoding* step applied to the
    /// schema text at parse time, before any argument exists.
    ///
    /// `None` covers two different situations, which
    /// [`has_sql_attribute`](Self::has_sql_attribute) distinguishes: no
    /// `@@sql` at all, or one whose argument is not a quoted string.
    /// Callers must not treat the second as an empty body — that is
    /// exactly the bug cratestack#867's review found, where
    /// `@@sql(SELECT 1)` compiled to `SQL = ""` with every `$N` check
    /// skipped.
    pub fn sql(&self) -> Option<String> {
        // `trim_start` matches `has_sql_attribute` exactly; see `View`'s
        // `body_attribute` for why the two must agree.
        self.attributes
            .iter()
            .map(|attr| attr.raw.trim_start())
            .filter(|raw| raw.starts_with(QUERY_SQL_ATTRIBUTE))
            .find_map(|raw| extract_sql_body(raw, QUERY_SQL_ATTRIBUTE))
    }

    /// Whether a `@@sql` attribute is written at all, however malformed.
    /// See [`sql`](Self::sql) for why the distinction matters.
    pub fn has_sql_attribute(&self) -> bool {
        self.attributes
            .iter()
            .any(|attr| attr.raw.trim_start().starts_with(QUERY_SQL_ATTRIBUTE))
    }
}

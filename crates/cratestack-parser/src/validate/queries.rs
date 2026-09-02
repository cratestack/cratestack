//! `query` block semantic checks (cratestack#867; accepted design
//! `docs/design/declarative-custom-query.md`).
//!
//! Each rule is enforced independently and produces a span-pointed
//! `SchemaError`. Rules:
//!
//! 1. Query names are unique, are valid Rust identifiers, and do not
//!    collide with one another after `to_snake_case` normalization (two
//!    queries would otherwise generate the same module).
//! 2. Exactly one `@@sql(…)` body. `@@server_sql`/`@@embedded_sql` are
//!    rejected outright — `query` is Postgres-only and has no per-backend
//!    split (design §4), so accepting the spelling would imply a backend
//!    that does not exist.
//! 3. Only `@@sql`, `@allow` and `@deny` are recognised attributes.
//! 4. Signature checks — see [`super::query_signature`].
//! 5. Positional-placeholder checks — see [`super::query_placeholders`].
//! 6. A `query` needs a database, so it is rejected under
//!    `datasource { provider = "none" }`.
//!
//! Deliberately **not** checked here, and this is the design's explicit
//! position rather than an omission (design §3): the correspondence
//! between the declared result `type` and the SQL's actual `SELECT` list.
//! Verifying it needs a live catalogue at macro-expansion time; a
//! mismatch instead surfaces loudly as `sqlx::Error::ColumnNotFound` /
//! `ColumnDecode` at first execution — exactly `view`'s shipped
//! behaviour, not a new gap.

use std::collections::BTreeSet;

use cratestack_core::route_naming::to_snake_case;
use cratestack_core::{Query, QUERY_SQL_ATTRIBUTE, Schema};

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::collect::record;
use crate::validate::query_placeholders::validate_query_placeholders;
use crate::validate::query_signature::{validate_query_args, validate_query_result_type};
use crate::validate::reserved_idents::validate_reserved_identifier;
use crate::validate::snake_case_collisions::find_collision_by;

/// Attributes a `query` block understands. Anything else is rejected so a
/// misspelled `@alow` fails loudly instead of silently leaving the query
/// deny-by-default with no explanation.
const RECOGNISED_ATTRIBUTES: &[&str] = &[QUERY_SQL_ATTRIBUTE, "@allow", "@deny"];

/// Each query is checked independently so one bad query does not hide the
/// next — same convention as `validate_views_collecting`.
pub(super) fn validate_queries_collecting(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    errors: &mut Vec<SchemaError>,
) {
    record(errors, || validate_unique_query_names(schema));
    record(errors, || validate_query_module_collisions(schema));
    record(errors, || validate_no_queries_under_datasource_none(schema));

    for query in &schema.queries {
        record(errors, || {
            validate_reserved_identifier(&query.name, query.name_span, &format!("query `{}`", query.name))
        });
        record(errors, || validate_query_attributes(query));
        record(errors, || validate_query_args(query));
        record(errors, || {
            validate_query_result_type(query, schema, type_names)
        });
        record(errors, || validate_query_placeholders(query));
    }
}

fn validate_unique_query_names(schema: &Schema) -> Result<(), SchemaError> {
    let mut seen = BTreeSet::new();
    for query in &schema.queries {
        if !seen.insert(query.name.as_str()) {
            return Err(span_error(
                format!("duplicate query name `{}`", query.name),
                query.span,
            ));
        }
    }
    Ok(())
}

/// Two queries whose names differ only in case/underscores generate the
/// same `pub mod <snake_case>` and the same accessor method, so the second
/// would silently shadow — or fail to compile with an error pointing at
/// generated code rather than at the schema.
fn validate_query_module_collisions(schema: &Schema) -> Result<(), SchemaError> {
    let entries = schema
        .queries
        .iter()
        .map(|query| (query.name.as_str(), query.span));
    match find_collision_by(entries, to_snake_case) {
        Some((first, second, span, normalized)) => Err(span_error(
            format!(
                "queries `{first}` and `{second}` both generate the module `{normalized}` — \
                 rename one so each query has its own generated module"
            ),
            span,
        )),
        None => Ok(()),
    }
}

fn validate_query_attributes(query: &Query) -> Result<(), SchemaError> {
    let mut sql_bodies = 0usize;
    for attribute in &query.attributes {
        let name = attribute_name(&attribute.raw);
        if matches!(name, "@@server_sql" | "@@embedded_sql") {
            return Err(span_error(
                format!(
                    "query `{}` declares `{name}`, but a `query` block has no per-backend SQL \
                     split: it is Postgres-only and takes its body from `@@sql(\"…\")`. If this \
                     query needs to run on the embedded backend, it cannot — see \
                     docs/design/declarative-custom-query.md §4",
                    query.name
                ),
                attribute.span,
            ));
        }
        if !RECOGNISED_ATTRIBUTES.contains(&name) {
            return Err(span_error(
                format!(
                    "query `{}` declares unsupported attribute `{name}` (a query understands \
                     `@@sql`, `@allow` and `@deny`)",
                    query.name
                ),
                attribute.span,
            ));
        }
        if name == QUERY_SQL_ATTRIBUTE {
            sql_bodies += 1;
        }
    }

    match sql_bodies {
        1 => Ok(()),
        0 => Err(span_error(
            format!(
                "query `{}` has no SQL body — add `@@sql(\"…\")` (or a `\"\"\"…\"\"\"` block for \
                 multiple lines)",
                query.name
            ),
            query.span,
        )),
        found => Err(span_error(
            format!(
                "query `{}` declares {found} `@@sql` bodies; exactly one is allowed",
                query.name
            ),
            query.span,
        )),
    }
}

/// The attribute's name, i.e. everything before its argument list.
fn attribute_name(raw: &str) -> &str {
    let trimmed = raw.trim();
    match trimmed.find('(') {
        Some(index) => trimmed[..index].trim_end(),
        None => trimmed,
    }
}

/// `provider = "none"` declares a procedures-only schema with no database
/// configured (cratestack#327). A `query` is nothing but SQL against a
/// database, so it cannot exist there — rejected at parse time with a
/// message that says which of the two to change, rather than surfacing as
/// a missing-`pool()` error inside generated code.
fn validate_no_queries_under_datasource_none(schema: &Schema) -> Result<(), SchemaError> {
    if super::datasource_provider(schema) != Some("none") {
        return Ok(());
    }
    match schema.queries.first() {
        Some(query) => Err(span_error(
            format!(
                "query `{}` is not allowed: schema declares `datasource {{ provider = \"none\" }}`, \
                 which configures no database for a `query` to run against",
                query.name
            ),
            query.span,
        )),
        None => Ok(()),
    }
}

//! `include_embedded_schema!` rejects any `query` block at expansion time
//! (cratestack#867; accepted design §4).
//!
//! Not a limitation waiting to be lifted — a deliberate scope boundary.
//! The escape hatch exists to write the Postgres spellings the declarative
//! read path cannot express (`FILTER (WHERE …)`, `::bigint`, window
//! functions, CTEs), and none of those are things a "portable custom
//! query" could credibly translate to SQLite. `cratestack-sql`'s
//! dialect-agnostic layer exists for the AST the framework *generates*; it
//! was never a target a hand-authored SQL string could be checked against.
//!
//! If embedded support is ever wanted, the design names the honest shape
//! so it is not invented ad hoc later: a second, independent
//! `@@embedded_sql`-style body on the same `query` block — two
//! dialect-specific strings the author writes — never one "portable"
//! syntax. The parser already rejects `@@embedded_sql` on a `query` today
//! (`cratestack-parser`'s `validate_query_attributes`), so that door is
//! closed at both ends rather than half-open.
//!
//! Mirrors [`super::computed_guard`]: fail once, here, naming the offending
//! blocks, rather than letting `query` silently vanish from the embedded
//! output — which is what would otherwise happen, since the embedded
//! composer simply never iterates `schema.queries`.

use proc_macro::TokenStream;
use syn::LitStr;

/// Every `query` block name, in declaration order.
fn query_names(schema: &cratestack_core::Schema) -> Vec<&str> {
    schema
        .queries
        .iter()
        .map(|query| query.name.as_str())
        .collect()
}

pub(super) fn guard_embedded_no_queries(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    let offenders = query_names(schema);
    if offenders.is_empty() {
        return Ok(());
    }
    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            format!(
                "schema declares `query` block(s) ({}), which are Postgres-only raw SQL; \
                 include_embedded_schema! has no Postgres backend and there is no \
                 `@@embedded_sql` twin for a query — remove the query blocks, or consume this \
                 schema through include_server_schema!(..., db = Postgres). See \
                 docs/design/declarative-custom-query.md §4.",
                offenders.join(", "),
            ),
        )
        .to_compile_error(),
    ))
}

#[cfg(test)]
mod tests {
    use super::query_names;

    // Same constraint as `computed_guard`'s tests: the guard itself calls
    // `syn::Error::to_compile_error()`, which panics outside an active
    // proc-macro invocation, so the predicate is what is exercised here.
    // The guard's real diagnostic is pinned by the `query_rejected_on_
    // embedded` trybuild case in `tests/ui_semantic.rs`.

    #[test]
    fn flags_every_query_in_declaration_order() {
        let schema = cratestack_parser::parse_schema(
            r#"
type Totals {
  total Int
}

query first(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)

query second(userId: String): Totals
  @@sql("SELECT 2 AS total WHERE a = $1")
  @allow(auth() != null)
"#,
        )
        .expect("schema should parse");

        assert_eq!(query_names(&schema), vec!["first", "second"]);
    }

    #[test]
    fn does_not_flag_a_schema_without_queries() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Account {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert!(query_names(&schema).is_empty());
    }
}

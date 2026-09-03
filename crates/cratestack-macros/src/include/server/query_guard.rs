//! `include_server_schema!(..., db = None)` rejects any `query` block at
//! expansion time (cratestack#867).
//!
//! The parser already rejects a `query` under an explicit `datasource
//! { provider = "none" }`, but that is not the same condition: a schema
//! with **no `datasource` block at all** is legal and passes
//! [`guard_server_datasource_provider`](super::super::datasource_guard),
//! which only cross-checks the `db` argument against a provider that is
//! actually declared. So `db = None` plus no datasource block reaches
//! codegen with queries intact, and would emit `db.pool()` against the
//! database-free `Cratestack` from `runtime/none.rs` — a wall of
//! "no method named `pool`" errors pointing inside a macro expansion.
//!
//! Same shape as `include::embedded::computed_guard`: fail once, here,
//! with a message that names the offending block and says what to change.

use proc_macro::TokenStream;
use syn::LitStr;

use super::super::parse::ServerDb;

pub(super) fn guard_no_queries_without_a_database(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
    db: ServerDb,
) -> Result<(), TokenStream> {
    if db != ServerDb::None {
        return Ok(());
    }
    let names = schema
        .queries
        .iter()
        .map(|query| query.name.as_str())
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Ok(());
    }
    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            format!(
                "schema declares `query` block(s) ({}) but this macro call says `db = None`, \
                 which configures no database — a `query` is raw SQL against Postgres and has \
                 nothing to run against. Either remove the query blocks, or switch this call to \
                 `db = Postgres` (and the schema's `datasource` to `provider = \"postgresql\"`). \
                 See docs/design/declarative-custom-query.md §4.",
                names.join(", "),
            ),
        )
        .to_compile_error(),
    ))
}

#[cfg(test)]
mod tests {
    // `guard_no_queries_without_a_database` returns a
    // `proc_macro::TokenStream` and calls `syn::Error::to_compile_error()`,
    // which panics outside an active proc-macro invocation — the same
    // constraint `datasource_guard`'s tests document. So the *predicate*
    // (does this schema declare any query?) is what is exercised directly
    // here; the guard's real compile-time behaviour is exercised by a
    // `trybuild`-style consumer in `cratestack-pg`'s fixtures.

    fn query_names(schema: &cratestack_core::Schema) -> Vec<&str> {
        schema
            .queries
            .iter()
            .map(|query| query.name.as_str())
            .collect()
    }

    #[test]
    fn finds_the_query_a_db_none_schema_must_not_have() {
        let schema = cratestack_parser::parse_schema(
            r#"
type Totals {
  total Int
}

query totals(userId: String): Totals
  @@sql("SELECT 1 AS total WHERE a = $1")
  @allow(auth() != null)
"#,
        )
        .expect("schema should parse");

        assert_eq!(query_names(&schema), vec!["totals"]);
    }

    #[test]
    fn a_schema_without_queries_has_nothing_to_report() {
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

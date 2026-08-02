//! Cross-checks `include_server_schema!`'s `db = Postgres` / `db = None`
//! argument against the schema's own `datasource { provider = "..." }` —
//! split out of `parse.rs` (which stays focused on entry-macro argument
//! parsing + the shared schema loader) per the repo's 200-LoC file
//! convention, mirroring `reject_grpc.rs`.
//!
//! Before cratestack#327, `args.db` was parsed and then discarded
//! (`let _ = args.db;` in `include.rs`) — nothing cross-checked it against
//! the schema's `datasource.provider`. A schema declaring `datasource {
//! provider = "none" }` paired with a stale `db = Postgres` macro call (or
//! vice versa) would silently do the wrong thing instead of failing loudly.
//! This guard closes that gap for both directions, for the first time ever
//! — including for the pre-existing `db = Postgres` case, not just the new
//! `db = None` one.
//!
//! A schema with no `datasource` block at all is left alone: the block is
//! optional today, and plenty of existing fixtures (see the cratestack#327
//! PR's audit) rely on `db = Postgres` with no explicit `datasource`.

use proc_macro::TokenStream;
use syn::LitStr;

use super::parse::ServerDb;

/// `include_server_schema!` only. See the module doc.
pub(super) fn guard_server_datasource_provider(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
    db: ServerDb,
) -> Result<(), TokenStream> {
    let Some(provider) = schema_datasource_provider(schema) else {
        // No `datasource` block at all: nothing to cross-check yet.
        return Ok(());
    };

    let expected_provider = match db {
        ServerDb::Postgres => "postgresql",
        ServerDb::None => "none",
    };

    if provider == expected_provider {
        return Ok(());
    }

    let db_arg = match db {
        ServerDb::Postgres => "Postgres",
        ServerDb::None => "None",
    };

    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            format!(
                "include_server_schema!(..., db = {db_arg}) requires this schema's `datasource \
                 {{ provider = \"...\" }}` to be `\"{expected_provider}\"`, but found \
                 `\"{provider}\"` — the macro's `db` argument and the schema's own `datasource` \
                 declaration must agree. Either change `db = {db_arg}` to match the schema, or \
                 change the schema's `provider` to `\"{expected_provider}\"` (see \
                 https://github.com/cratestack/cratestack/issues/327)."
            ),
        )
        .to_compile_error(),
    ))
}

fn schema_datasource_provider(schema: &cratestack_core::Schema) -> Option<&str> {
    schema
        .datasource
        .as_ref()?
        .entries
        .iter()
        .find(|entry| entry.key == "provider")
        .map(|entry| entry.value.trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use super::{ServerDb, schema_datasource_provider};

    // Same constraint as `reject_grpc.rs`'s tests: `guard_server_datasource_provider`
    // returns `proc_macro::TokenStream` and calls `syn::Error::to_compile_error()`,
    // which panics outside an active proc-macro invocation context. So the pure
    // `schema_datasource_provider` predicate is exercised directly here, and the
    // guard's actual compile-time behavior (both the pass and fail cases, for both
    // `db = Postgres` and `db = None`) is exercised by real `include_server_schema!`
    // call sites in `cratestack-pg`'s own test/example fixtures.

    #[test]
    fn reads_provider_from_datasource_block() {
        let schema = cratestack_parser::parse_schema(
            r#"
datasource db {
  provider = "postgresql"
}

model Widget {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert_eq!(schema_datasource_provider(&schema), Some("postgresql"));
    }

    #[test]
    fn reads_none_provider_from_datasource_block() {
        let schema = cratestack_parser::parse_schema(
            r#"
datasource db {
  provider = "none"
}
"#,
        )
        .expect("schema should parse");

        assert_eq!(schema_datasource_provider(&schema), Some("none"));
    }

    #[test]
    fn no_datasource_block_yields_no_provider() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Widget {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert_eq!(schema_datasource_provider(&schema), None);
    }

    #[test]
    fn server_db_variants_are_distinct() {
        assert_ne!(ServerDb::Postgres, ServerDb::None);
    }
}

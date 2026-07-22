//! Argument parsing for the three top-level include macros + shared
//! schema file loader. `include_server_schema!` takes `db = Postgres`
//! (only Postgres is wired today); `include_embedded_schema!` and
//! `include_client_schema!` take a bare path literal.

use std::path::PathBuf;

use proc_macro::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::{LitStr, Token};

/// Supported sqlx database backends for `include_server_schema!`.
///
/// Today only `Postgres` is accepted; the parser is wired so adding
/// `MySql` / `Sqlite`-via-sqlx (when we want them) is a non-breaking
/// change at call sites that already pass `db = Postgres`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerDb {
    Postgres,
}

/// Parsed arguments for `include_server_schema!("schema.cstack", db = Postgres)`.
pub(super) struct ServerSchemaArgs {
    pub(super) schema_path: LitStr,
    pub(super) db: ServerDb,
}

impl Parse for ServerSchemaArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let schema_path: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let key: syn::Ident = input.parse()?;
        if key != "db" {
            return Err(syn::Error::new(
                key.span(),
                "expected `db = Postgres` (only the `db` argument is recognised)",
            ));
        }
        input.parse::<Token![=]>()?;
        let value: syn::Ident = input.parse()?;
        let db = match value.to_string().as_str() {
            "Postgres" => ServerDb::Postgres,
            other => {
                return Err(syn::Error::new(
                    value.span(),
                    format!(
                        "unsupported db backend `{other}`. supported: Postgres. (MySql / sqlite-via-sqlx will land in a future release.)"
                    ),
                ));
            }
        };
        Ok(Self { schema_path, db })
    }
}

pub(super) fn parse_schema_literal(
    schema_path: &LitStr,
) -> Result<(String, PathBuf, cratestack_core::Schema), TokenStream> {
    let schema_relative = schema_path.value();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let resolved = PathBuf::from(&manifest_dir).join(&schema_relative);
    let source = std::fs::read_to_string(&resolved).map_err(|error| {
        TokenStream::from(
            syn::Error::new(
                schema_path.span(),
                format!("failed to read schema file {}: {error}", resolved.display()),
            )
            .to_compile_error(),
        )
    })?;

    let schema = cratestack_parser::parse_schema_named(&resolved.display().to_string(), &source)
        .map_err(|error| {
            TokenStream::from(
                syn::Error::new(
                    schema_path.span(),
                    error.render(&resolved.display().to_string(), &source),
                )
                .to_compile_error(),
            )
        })?;

    reject_composite_primary_keys(schema_path, &schema)?;

    Ok((schema_relative, resolved, schema))
}

/// `@@id([...])` composite primary keys are parsed and validated by
/// `cratestack-parser`, and `cratestack-migrate` already emits correct
/// composite `PRIMARY KEY` DDL for them — but query builders, axum/RPC
/// routing, and all three client generators still assume exactly one
/// scalar PK column throughout (`ModelDescriptor<M, PK>` and friends).
/// Fail here with one clear message instead of letting a model with
/// `@@id(...)` reach codegen and panic somewhere deep in a `.find(...)
/// .expect(...)` call with no useful context.
///
/// Tracking: <https://github.com/cratestack/cratestack/issues/136>.
fn reject_composite_primary_keys(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    if let Some(model) = find_composite_id_model(schema) {
        return Err(TokenStream::from(
            syn::Error::new(
                schema_path.span(),
                format!(
                    "model `{}` declares a composite primary key via `@@id([...])`, which is not yet supported by codegen (query builders, routing, and generated clients still assume a single scalar `@id`); see https://github.com/cratestack/cratestack/issues/136 for status",
                    model.name,
                ),
            )
            .to_compile_error(),
        ));
    }
    Ok(())
}

fn find_composite_id_model(schema: &cratestack_core::Schema) -> Option<&cratestack_core::Model> {
    schema
        .models
        .iter()
        .find(|model| model.attributes.iter().any(|a| a.raw.starts_with("@@id(")))
}

#[cfg(test)]
mod tests {
    use super::find_composite_id_model;

    #[test]
    fn flags_model_with_composite_id_attribute() {
        let schema = cratestack_parser::parse_schema(
            r#"
model AccountMembership {
  accountId Int
  subject String

  @@id([accountId, subject])
}
"#,
        )
        .expect("schema should parse");

        let flagged = find_composite_id_model(&schema);
        assert_eq!(
            flagged.map(|model| model.name.as_str()),
            Some("AccountMembership")
        );
    }

    #[test]
    fn does_not_flag_single_field_id() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Account {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert!(find_composite_id_model(&schema).is_none());
    }
}

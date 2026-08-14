//! Macro-argument shapes for the three entry macros. Split out of
//! `parse.rs` to keep both files under this repo's ~200-LoC convention —
//! `parse.rs` keeps the schema-file loading/hashing/composite-id-rejection
//! side, this file keeps the `syn::parse::Parse` argument grammars.

use syn::parse::{Parse, ParseStream};
use syn::{LitStr, Token};

use super::decimal_arg::parse_optional_decimal_arg;
use crate::shared::decimal_backend::DecimalBackend;

/// Supported `db` arguments for `include_server_schema!`.
///
/// `Postgres` is the sqlx-backed database mode; `None` is cratestack#327's
/// "no database" procedures-only mode. Both are cross-checked against the
/// schema's own `datasource.provider` (`postgresql` / `none` respectively)
/// by [`super::datasource_guard::guard_server_datasource_provider`] — a
/// mismatch is a compile-time error, not silently ignored. The parser is
/// wired so adding `MySql` / `Sqlite`-via-sqlx (when we want them) is a
/// non-breaking change at call sites that already pass `db = Postgres`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerDb {
    Postgres,
    None,
}

/// Parsed arguments for `include_server_schema!("schema.cstack", db = Postgres)`,
/// optionally followed by `, decimal = RustDecimal | BigDecimal`
/// (cratestack#505 Direction 2 — see `super::decimal_arg`'s module doc;
/// required only when the schema declares a `Decimal` field).
pub(super) struct ServerSchemaArgs {
    pub(super) schema_path: LitStr,
    pub(super) db: ServerDb,
    pub(super) decimal: Option<DecimalBackend>,
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
            "None" => ServerDb::None,
            other => {
                return Err(syn::Error::new(
                    value.span(),
                    format!(
                        "unsupported db backend `{other}`. supported: Postgres, None. (MySql / sqlite-via-sqlx will land in a future release.)"
                    ),
                ));
            }
        };
        let decimal = parse_optional_decimal_arg(input)?;
        Ok(Self {
            schema_path,
            db,
            decimal,
        })
    }
}

/// Parsed arguments for `include_embedded_schema!("schema.cstack")` /
/// `include_client_schema!("schema.cstack")`: a bare path literal,
/// optionally followed by `, decimal = RustDecimal | BigDecimal` — the
/// same argument `ServerSchemaArgs` accepts after `db = ...`, see its doc.
pub(super) struct SchemaPathArgs {
    pub(super) schema_path: LitStr,
    pub(super) decimal: Option<DecimalBackend>,
}

impl Parse for SchemaPathArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let schema_path: LitStr = input.parse()?;
        let decimal = parse_optional_decimal_arg(input)?;
        Ok(Self {
            schema_path,
            decimal,
        })
    }
}

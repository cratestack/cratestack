//! Macro-argument shapes for the three entry macros. Split out of
//! `parse.rs` to keep both files under this repo's ~200-LoC convention —
//! `parse.rs` keeps the schema-file loading/hashing/composite-id-rejection
//! side, this file keeps the `syn::parse::Parse` argument grammars.

use syn::parse::{Parse, ParseStream};
use syn::{LitStr, Token};

use super::decimal_arg::{parse_decimal_value, parse_optional_decimal_arg};
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
///
/// `decimal` and `contracts = "schema.contracts.lock"` (cratestack#1123:
/// the compatible-contract lock a server accepts older signed clients
/// from) may follow `db`, in either order, each at most once.
pub(super) struct ServerSchemaArgs {
    pub(super) schema_path: LitStr,
    pub(super) db: ServerDb,
    pub(super) decimal: Option<DecimalBackend>,
    pub(super) contracts: Option<LitStr>,
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
        let (decimal, contracts) = parse_server_options(input)?;
        Ok(Self {
            schema_path,
            db,
            decimal,
            contracts,
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

/// The optional `, decimal = ...` and `, contracts = "..."` after `db`.
fn parse_server_options(
    input: ParseStream<'_>,
) -> syn::Result<(Option<DecimalBackend>, Option<LitStr>)> {
    let (mut decimal, mut contracts) = (None, None);
    while !input.is_empty() {
        input.parse::<Token![,]>()?;
        if input.is_empty() {
            break;
        }
        let key: syn::Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        match key.to_string().as_str() {
            "decimal" if decimal.is_none() => decimal = Some(parse_decimal_value(input)?),
            "contracts" if contracts.is_none() => contracts = Some(input.parse::<LitStr>()?),
            "decimal" | "contracts" => {
                return Err(syn::Error::new(
                    key.span(),
                    format!("`{key}` is given twice"),
                ));
            }
            _ => {
                return Err(syn::Error::new(
                    key.span(),
                    "expected `decimal = RustDecimal | BigDecimal` or \
                     `contracts = \"schema.contracts.lock\"`",
                ));
            }
        }
    }
    Ok((decimal, contracts))
}

#[cfg(test)]
mod tests {
    use super::{ServerDb, ServerSchemaArgs};

    fn parse(tokens: &str) -> syn::Result<ServerSchemaArgs> {
        syn::parse_str(tokens)
    }

    #[test]
    fn contracts_and_decimal_follow_db_in_either_order() {
        let a = parse(r#""s.cstack", db = Postgres, contracts = "s.lock", decimal = BigDecimal"#)
            .unwrap();
        let b =
            parse(r#""s.cstack", db = None, decimal = RustDecimal, contracts = "s.lock""#).unwrap();
        assert_eq!(a.contracts.unwrap().value(), "s.lock");
        assert_eq!(b.contracts.unwrap().value(), "s.lock");
        assert!(a.decimal.is_some() && b.decimal.is_some());
        assert_eq!((a.db, b.db), (ServerDb::Postgres, ServerDb::None));
    }

    #[test]
    fn the_options_are_optional_and_a_trailing_comma_is_fine() {
        let plain = parse(r#""s.cstack", db = None"#).unwrap();
        assert!(plain.contracts.is_none() && plain.decimal.is_none());
        assert!(parse(r#""s.cstack", db = None, contracts = "s.lock","#).is_ok());
    }

    #[test]
    fn a_repeated_or_unknown_option_is_refused() {
        let twice = parse(r#""s.cstack", db = None, contracts = "a", contracts = "b""#);
        assert!(twice.err().unwrap().to_string().contains("given twice"));
        let unknown = parse(r#""s.cstack", db = None, lock = "a""#);
        assert!(unknown.err().unwrap().to_string().contains("contracts ="));
        assert!(parse(r#""s.cstack", db = None, contracts = 3"#).is_err());
    }
}

//! `impl sqlx::FromRow<'_, PgRow>` for every `type` a `query` returns
//! (cratestack#867). Server-only — a `query` never reaches the embedded or
//! client composers at all.
//!
//! The per-field decode is `view::row_field_tokens`, reused rather than
//! reimplemented: design §3 chooses `view`'s exact shape (column-name
//! `try_get` against the declared field name, enums parsed from their
//! string form) as the precedent, so sharing the function is what makes
//! "identical to a view's" true rather than merely intended.
//!
//! **Column names are the declared field names, verbatim.** A `type`
//! field `thisMonth` decodes from a column literally named `thisMonth`, so
//! the author's SQL must alias it `AS "thisMonth"` — quoted, because
//! Postgres folds unquoted identifiers to lower case. Unlike `view`, whose
//! `SELECT` the framework generates (and aliases automatically), a
//! `query`'s `SELECT` is the author's, so there is nowhere to insert the
//! alias for them. Deliberately no snake_case fallback: a decode that
//! tries two names is a decode whose failure mode depends on which of the
//! two the author happened to write, and design §3's whole position is
//! that a mismatch must fail loudly (`sqlx::Error::ColumnNotFound`) rather
//! than resolve ambiguously.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Query, TypeDecl};
use quote::quote;

use crate::shared::ident;
use crate::view::row_field_tokens;

/// One `FromRow` impl per distinct result `type` across every `query`.
///
/// De-duplicated by type name: two queries returning the same `type` would
/// otherwise emit two identical impls, which is a conflicting-implementation
/// error rather than a harmless duplicate.
pub(crate) fn generate_query_result_from_row_impls(
    queries: &[Query],
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> Vec<proc_macro2::TokenStream> {
    let mut wanted: BTreeMap<&str, &TypeDecl> = BTreeMap::new();
    for query in queries {
        let name = query.result_type.name.as_str();
        if let Some(decl) = types.iter().find(|candidate| candidate.name == name) {
            wanted.insert(name, decl);
        }
    }

    wanted
        .into_values()
        .map(|decl| from_row_impl(decl, enum_names))
        .collect()
}

fn from_row_impl(decl: &TypeDecl, enum_names: &BTreeSet<&str>) -> proc_macro2::TokenStream {
    let type_ident = ident(&decl.name);
    // `row_field_tokens` emits enum paths as `super::types::<Enum>`,
    // which resolves from one level below `cratestack_schema` — the depth
    // `pub mod queries` sits at, the same as `pub mod models` where the
    // view impls land.
    let row_fields = decl
        .fields
        .iter()
        .map(|field| row_field_tokens(field, enum_names));

    quote! {
        impl<'r> ::cratestack::sqlx::FromRow<'r, ::cratestack::sqlx::postgres::PgRow>
            for super::types::#type_ident
        {
            fn from_row(
                row: &'r ::cratestack::sqlx::postgres::PgRow,
            ) -> Result<Self, ::cratestack::sqlx::Error> {
                use ::cratestack::sqlx::Row;
                Ok(Self {
                    #(#row_fields)*
                })
            }
        }
    }
}

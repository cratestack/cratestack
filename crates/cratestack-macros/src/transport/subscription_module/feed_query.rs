//! The generated `feed_query`: a model feed's input, parsed once per
//! subscription with `list`'s own parser (design §3.1, §5.3, slice S3).

use quote::quote;

pub(super) fn tokens() -> proc_macro2::TokenStream {
    quote! {
            /// A model feed's query, parsed (design §3.1, §5.3).
            struct FeedQuery {
                operations: Option<Vec<::cratestack::ModelEventKind>>,
                filter: ::cratestack::FilterExpr,
                fields: Option<Vec<String>>,
            }

            /// `list`'s filter syntax, parsed by `list`'s own parser, plus
            /// `operations`. A filter that cannot be decided in memory (a
            /// relation, JSON path…) or names a column the event record does
            /// not carry is refused here, before the feed opens.
            fn feed_query(
                query: Option<&str>,
                build: fn(&::cratestack::QueryExpr) -> Result<::cratestack::FilterExpr, ::cratestack::CratestackError>,
                record_columns: &[&str],
                allowed_fields: &[&str],
            ) -> Result<FeedQuery, ::cratestack::CratestackError> {
                let ::cratestack::__private::SubscribeQuery { operations, rest } =
                    ::cratestack::__private::split_subscribe_query(query)?;
                let list = super::axum::parse_model_list_query(rest.as_deref())?;
                let filter = ::cratestack::FilterExpr::all(
                    list.filters.iter().map(build).collect::<Result<Vec<_>, _>>()?,
                );
                let bad = |what: String| ::cratestack::CratestackError::BadRequest(format!(
                    "{what} cannot filter a subscription",
                ));
                let columns = ::cratestack::__private::filter_in_memory_columns(&filter)
                    .map_err(|what| bad(what.to_owned()))?;
                if let Some(column) = columns.into_iter().find(|column| !record_columns.contains(column)) {
                    return Err(bad(format!("`{column}`")));
                }
                let fields = list.selection.fields;
                if let Some(unknown) = fields
                    .iter()
                    .flatten()
                    .find(|field| !allowed_fields.contains(&field.as_str()))
                {
                    return Err(::cratestack::CratestackError::BadRequest(format!(
                        "unknown field `{unknown}`",
                    )));
                }
                Ok(FeedQuery { operations, filter, fields })
            }
    }
}

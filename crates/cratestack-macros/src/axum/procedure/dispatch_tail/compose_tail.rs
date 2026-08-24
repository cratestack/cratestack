//! [`compose_tail_tokens`] — split out of the parent `dispatch_tail`
//! module per the repo's 200-LoC file convention. See the call site in
//! `super::procedure_dispatch_tail_tokens` for how this fits into the
//! rest of the dispatch tail.

use quote::quote;

use crate::computed::{ProcedureOutputComposition, compose_fn_ident};

/// Rebinds `result` (shadowing the `Result<Output, CratestackError>`
/// `invoke_with_db` produced) into the composed
/// `::cratestack::ProjectedValue` shape a computed-bearing output needs
/// on the wire (`docs/design/computed-fields.md`'s "Procedure outputs"
/// section) — empty for every procedure whose output isn't computed-
/// bearing, so those keep emitting byte-identical dispatch code (see
/// `crate::axum::procedure::tests`'s pinned non-computed regression
/// guard).
///
/// `result?` inside the `async { ... }` block scopes to that block, not
/// the enclosing `handle_<procedure>_dispatch` fn (which returns
/// `Response`, not `Result<_, _>`) — the same way it would inside any
/// other closure or async block; only a resolver failure or a compose-
/// time propagation reaches the `Err` arm the encoder below already
/// knows how to map to a response, so a compose failure gets the exact
/// same status/tracing treatment as any other procedure error.
pub(super) fn compose_tail_tokens(
    composition: Option<ProcedureOutputComposition>,
) -> proc_macro2::TokenStream {
    let Some(composition) = composition else {
        return quote! {};
    };

    match composition {
        ProcedureOutputComposition::Unary { owner, optional } => {
            let compose_ident = compose_fn_ident(&owner);
            let body = if optional {
                quote! {
                    match output {
                        ::core::option::Option::Some(value) => {
                            #compose_ident(&state.db, &state.resolvers, &ctx, &value).await
                        }
                        ::core::option::Option::None => Ok(::cratestack::ProjectedValue::Null),
                    }
                }
            } else {
                quote! {
                    #compose_ident(&state.db, &state.resolvers, &ctx, &output).await
                }
            };
            quote! {
                let result: Result<::cratestack::ProjectedValue, CratestackError> = async {
                    let output = result?;
                    #body
                }.await;
            }
        }
        ProcedureOutputComposition::List { owner } => {
            let compose_ident = compose_fn_ident(&owner);
            quote! {
                let result: Result<::std::vec::Vec<::cratestack::ProjectedValue>, CratestackError> = async {
                    let items = result?;
                    let mut composed = ::std::vec::Vec::with_capacity(items.len());
                    for item in &items {
                        composed.push(#compose_ident(&state.db, &state.resolvers, &ctx, item).await?);
                    }
                    Ok(composed)
                }.await;
            }
        }
        ProcedureOutputComposition::Page { owner } => {
            let compose_ident = compose_fn_ident(&owner);
            quote! {
                let result: Result<::cratestack::ProjectedValue, CratestackError> = async {
                    let page = result?;
                    let mut items = ::std::vec::Vec::with_capacity(page.items.len());
                    for item in &page.items {
                        items.push(#compose_ident(&state.db, &state.resolvers, &ctx, item).await?);
                    }
                    // Mirrors `cratestack_core::Page<T>`'s own
                    // `#[serde(rename_all = "camelCase")]` shape exactly
                    // (`items`/`totalCount`/`pageInfo`) — a wrong key here
                    // would be a silent wire break the generated client's
                    // `Page<T>` decode would fail on.
                    let mut object = ::std::collections::BTreeMap::new();
                    object.insert("items".to_owned(), ::cratestack::ProjectedValue::Array(items));
                    object.insert(
                        "totalCount".to_owned(),
                        ::cratestack::ProjectedValue::leaf(page.total_count),
                    );
                    object.insert(
                        "pageInfo".to_owned(),
                        ::cratestack::ProjectedValue::leaf(page.page_info),
                    );
                    Ok(::cratestack::ProjectedValue::Object(object))
                }.await;
            }
        }
    }
}

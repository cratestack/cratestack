//! `compose_<owner_snake>_value` helpers — one per computed-bearing
//! owner (`type` or `model`), turning `&Owner` into a
//! `::cratestack::ProjectedValue` with every `@computed` field resolved
//! (`docs/design/computed-fields.md`'s "Procedure outputs" section).
//! Consumed by the procedure dispatch tail
//! (`crate::axum::procedure::dispatch_tail`) for any procedure whose
//! `Output` reaches a computed-bearing owner —
//! [`super::bearing::procedure_output_composition`] decides which.
//!
//! Emitted once per schema, not once per procedure: several procedures
//! can return the same owner, and a nested owner (`type Card { cover
//! Image }`) is composed on behalf of *whichever* procedure returns
//! `Card`, not `Card`'s own declaration site — so these live at the
//! generated `axum` module's top level (spliced in by
//! `include::server::axum_module`) alongside the per-procedure handlers
//! that call them, rather than nested inside any one procedure's own
//! generated code.
//!
//! Field-by-field, each helper mirrors
//! `axum::model::serializers::projection_fields` (cratestack#430): a
//! stored field becomes `ProjectedValue::leaf(value.field.clone())` —
//! never routed through `serde_json::to_value` first, so a leaf's own
//! `Serialize` impl still sees the real target codec's `is_human_
//! readable()` at encode time — a computed field resolves through
//! `resolvers` and is wrapped the same way, and a field whose own type
//! is itself computed-bearing recurses into that owner's compose helper
//! (arity-aware: `Option<T>` maps `None` to `ProjectedValue::Null`,
//! `Vec<T>` maps to `ProjectedValue::Array`). No cycles are possible to
//! guard against here: a Rust struct field's type can never (transitively
//! or otherwise) name the struct that contains it — that would be an
//! infinite-size type — so two `compose_*_value` fns can only ever form
//! a DAG of calls, never call each other in a loop, and don't need the
//! `Pin<Box<dyn Future>>` indirection `serialize_<model>_model_value`
//! uses for models' genuinely-recursive self-relations.

use std::collections::BTreeSet;

use cratestack_core::{Field, Schema, TypeArity, computed_params_type_name};
use quote::quote;

use crate::shared::{ident, is_computed_field, wire_model_fields};

use super::bearing::compose_fn_ident;
use super::resolver_method_name;

/// One `object.insert(...)` per field of an owner's wire shape.
fn field_compose_tokens(
    owner_name: &str,
    field: &Field,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let field_name = &field.name;

    if is_computed_field(field) {
        let method_ident = ident(&resolver_method_name(owner_name, &field.name));
        let resolve_call = if computed_params_type_name(field).is_some() {
            // Procedure-context resolution always passes `params: None`
            // in v1 (`docs/design/computed-fields.md`'s "Response
            // composition" section) — there is no wire slot for
            // computed params on a procedure request.
            quote! { resolvers.#method_ident(db, value, None, ctx).await? }
        } else {
            quote! { resolvers.#method_ident(db, value, ctx).await? }
        };
        return quote! {
            object.insert(
                #field_name.to_owned(),
                ::cratestack::ProjectedValue::leaf(#resolve_call),
            );
        };
    }

    if bearing.contains(&field.ty.name) {
        let nested_ident = compose_fn_ident(&field.ty.name);
        let nested_expr = match field.ty.arity {
            TypeArity::Required => quote! {
                #nested_ident(db, resolvers, ctx, &value.#field_ident).await?
            },
            TypeArity::Optional => quote! {
                match &value.#field_ident {
                    ::core::option::Option::Some(inner) => {
                        #nested_ident(db, resolvers, ctx, inner).await?
                    }
                    ::core::option::Option::None => ::cratestack::ProjectedValue::Null,
                }
            },
            TypeArity::List => quote! {
                {
                    let mut items = ::std::vec::Vec::with_capacity(value.#field_ident.len());
                    for item in &value.#field_ident {
                        items.push(#nested_ident(db, resolvers, ctx, item).await?);
                    }
                    ::cratestack::ProjectedValue::Array(items)
                }
            },
        };
        return quote! {
            object.insert(#field_name.to_owned(), #nested_expr);
        };
    }

    quote! {
        object.insert(
            #field_name.to_owned(),
            ::cratestack::ProjectedValue::leaf(value.#field_ident.clone()),
        );
    }
}

fn owner_compose_fn(
    owner_name: &str,
    fields: &[&Field],
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let compose_ident = compose_fn_ident(owner_name);
    let owner_ident = ident(owner_name);
    let field_tokens: Vec<_> = fields
        .iter()
        .map(|field| field_compose_tokens(owner_name, field, bearing))
        .collect();

    quote! {
        async fn #compose_ident<CR: super::computed::ComputedFieldResolver>(
            db: &super::Cratestack,
            resolvers: &CR,
            ctx: &::cratestack::CratestackContext,
            value: &super::#owner_ident,
        ) -> Result<::cratestack::ProjectedValue, CratestackError> {
            let mut object = ::std::collections::BTreeMap::new();
            #(#field_tokens)*
            Ok(::cratestack::ProjectedValue::Object(object))
        }
    }
}

/// `compose_<owner_snake>_value` fn definitions for every computed-bearing
/// owner in `schema` — empty when the schema has none (zero generated
/// code for a schema with no `@computed` fields at all, matching every
/// other computed-fields codegen path in this crate).
pub(crate) fn generate_compose_helpers(
    schema: &Schema,
    model_names: &BTreeSet<&str>,
    bearing: &BTreeSet<String>,
) -> Vec<proc_macro2::TokenStream> {
    let mut helpers = Vec::new();

    for model in &schema.models {
        if !bearing.contains(&model.name) {
            continue;
        }
        let fields = wire_model_fields(model, model_names);
        helpers.push(owner_compose_fn(&model.name, &fields, bearing));
    }

    for ty in &schema.types {
        if !bearing.contains(&ty.name) {
            continue;
        }
        let fields: Vec<&Field> = ty.fields.iter().collect();
        helpers.push(owner_compose_fn(&ty.name, &fields, bearing));
    }

    helpers
}

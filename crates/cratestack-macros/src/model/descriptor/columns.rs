//! Per-column / per-field lists embedded in `ModelDescriptor`:
//! `columns`, `allowed_fields`, `allowed_includes`, `allowed_sorts`,
//! `pii_columns`, `sensitive_columns`, `upsert_update_columns`.

use cratestack_core::Model;
use quote::quote;

use crate::relation::collect_allowed_sort_keys;
use crate::shared::{
    is_generated_on_create, is_pii_field, is_primary_key, is_readonly_field, is_sensitive_field,
    is_server_only_field, is_version_field, model_name_set, relation_model_fields,
    scalar_model_fields, to_snake_case,
};

pub(super) struct ColumnLists {
    pub(super) columns: Vec<proc_macro2::TokenStream>,
    pub(super) allowed_fields: Vec<proc_macro2::TokenStream>,
    pub(super) allowed_includes: Vec<proc_macro2::TokenStream>,
    pub(super) allowed_sorts: Vec<proc_macro2::TokenStream>,
    pub(super) pii_columns: Vec<proc_macro2::TokenStream>,
    pub(super) sensitive_columns: Vec<proc_macro2::TokenStream>,
    pub(super) upsert_update_columns: Vec<proc_macro2::TokenStream>,
}

pub(super) fn collect_column_lists(model: &Model, models: &[Model]) -> Result<ColumnLists, String> {
    let model_names = model_name_set(models);
    let columns = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            let rust_name = &field.name;
            let sql_name = to_snake_case(&field.name);
            quote! {
                ::cratestack::ModelColumn {
                    rust_name: #rust_name,
                    sql_name: #sql_name,
                }
            }
        })
        .collect();
    let allowed_fields = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter(|field| !is_server_only_field(field))
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect();
    let allowed_includes = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect();
    let allowed_sorts = collect_allowed_sort_keys(model, models)?
        .into_iter()
        .map(|field| quote! { #field })
        .collect();
    let pii_columns = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter(|field| is_pii_field(field))
        .map(|field| {
            let column = to_snake_case(&field.name);
            quote! { #column }
        })
        .collect();
    let sensitive_columns = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter(|field| is_sensitive_field(field))
        .map(|field| {
            let column = to_snake_case(&field.name);
            quote! { #column }
        })
        .collect();
    // Columns the upsert primitive may overwrite on conflict. Excludes:
    //   - primary key (the conflict target — must not appear in SET)
    //   - `@version` (bumped server-side, never carried by input)
    //   - `@readonly` / `@server_only` (never settable from input)
    //   - `@default(...)` columns (server-owned identity bindings like
    //     auth-derived `ownership_id`; clobbering these on update would
    //     turn upsert into "take ownership of any row I name").
    //
    // The resulting set matches `Update{Model}Input`'s fields, which is
    // the right shape — we're treating upsert's update branch as a
    // forced overwrite of the caller-provided non-defaulted columns.
    let upsert_update_columns = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter(|f| !is_primary_key(f) && !is_version_field(f))
        .filter(|f| !is_readonly_field(f) && !is_server_only_field(f))
        .filter(|f| !is_generated_on_create(f))
        .map(|field| {
            let column = to_snake_case(&field.name);
            quote! { #column }
        })
        .collect();

    Ok(ColumnLists {
        columns,
        allowed_fields,
        allowed_includes,
        allowed_sorts,
        pii_columns,
        sensitive_columns,
        upsert_update_columns,
    })
}

//! Plain model `struct` emission (server + client variants). The shared
//! `struct_field_definition` field-token builder used by every struct +
//! input emitter lives in `struct_only/field_definition.rs`, split out
//! per the repo's 200-LoC file convention; re-exported here so
//! `crate::model::struct_only::{struct_field_type, struct_field_definition}`
//! call sites don't need to know about the split.

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::builder::{
    generate_builder, model_builder_fields, model_builder_fields_with_wire_scope,
};
use crate::shared::{
    doc_attrs, ident, is_primary_key, rust_type_tokens, scalar_model_fields, wire_model_fields,
};

mod field_definition;

pub(crate) use field_definition::{
    struct_field_definition, struct_field_definition_with_wire_scope, struct_field_type,
    struct_field_type_with_wire_scope,
};

/// Emit just the model `struct` (with serde derives) — no backend-specific
/// `FromRow` impls. Used by every composer.
pub(crate) fn generate_model_struct_only(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let docs = doc_attrs(&model.docs);
    let scalar_fields = scalar_model_fields(model, model_names);
    let fields = scalar_fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));
    let builder = generate_builder(
        &model_ident,
        &model_builder_fields(scalar_fields.iter().copied(), false, enum_names),
    );

    // `Default` is required so `.find_unique(id).select(...).run(ctx)`
    // can return a `Projection<T>` where non-selected fields hold
    // type defaults. The constraint propagates to every field type;
    // schemas with non-Default `Json<MyCustomStruct>` fields error at
    // the macro boundary and the fix is to derive Default on the
    // custom struct (or wrap the field in Option). For the standard
    // primitive set (i64 / String / bool / DateTime / Decimal / Uuid /
    // Vec<u8> / serde_json::Value / Option<T>) Default is already
    // available, so the change is invisible to most schemas.
    quote! {
        #docs
        #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #model_ident {
            #(#fields)*
        }

        #builder
    }
}

/// Emit `impl ModelPrimaryKey<PK> for Model`. Used by batch operations to
/// pair returned rows back to their input position. Backend-agnostic — same
/// impl works on server (sqlx) and embedded (rusqlite) since it only
/// touches in-memory model fields.
pub(crate) fn generate_primary_key_accessor_impl(model: &Model) -> proc_macro2::TokenStream {
    let primary_key = match model.fields.iter().find(|field| is_primary_key(field)) {
        Some(pk) => pk,
        // Validated schemas always have a primary key; this guard exists
        // only so the macro doesn't panic during partial-fixture tests.
        None => return quote! {},
    };
    let model_ident = ident(&model.name);
    let pk_type = rust_type_tokens(&primary_key.ty);
    let pk_field_ident = ident(&primary_key.name);
    quote! {
        impl ::cratestack::ModelPrimaryKey<#pk_type> for #model_ident {
            fn primary_key(&self) -> #pk_type {
                self.#pk_field_ident.clone()
            }
        }
    }
}

/// Client-side model struct — unlike [`generate_model_struct_only`], this
/// INCLUDES `@computed` fields: they are part of the wire response shape
/// even though the server never stores or hand-constructs them (see
/// `docs/design/computed-fields.md`).
pub(crate) fn generate_client_model_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let docs = doc_attrs(&model.docs);
    let wire_fields = wire_model_fields(model, model_names);
    let fields = wire_fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));
    let builder = generate_builder(
        &model_ident,
        &model_builder_fields(wire_fields.iter().copied(), false, enum_names),
    );

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #model_ident {
            #(#fields)*
        }

        #builder
    }
}

/// [`generate_client_model_struct`]'s wire-scope counterpart, for the
/// server composer's dedicated `wire` module (`crate::computed::wire`) —
/// emitted only for a computed-bearing model. Field-for-field identical
/// to `generate_client_model_struct` except a field whose own type is
/// *also* computed-bearing resolves to the sibling `super::wire::<Owner>`
/// struct instead of the plain server-side `super::<Owner>` — see
/// [`struct_field_type_with_wire_scope`]'s doc for why. `bearing` is the
/// schema-wide computed-bearing set (`crate::computed::computed_bearing_names`),
/// not just "is this model itself bearing" — a model's *field* can name a
/// different bearing owner (a `type` field referencing a `model`
/// directly, per `crate::types::generate_type_struct`'s doc on
/// `custom_in_super`).
pub(crate) fn generate_wire_model_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let docs = doc_attrs(&model.docs);
    let wire_fields = wire_model_fields(model, model_names);
    let fields = wire_fields
        .iter()
        .map(|field| struct_field_definition_with_wire_scope(field, enum_names, bearing));
    let builder = generate_builder(
        &model_ident,
        &model_builder_fields_with_wire_scope(wire_fields.iter().copied(), enum_names, bearing),
    );

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #model_ident {
            #(#fields)*
        }

        #builder
    }
}

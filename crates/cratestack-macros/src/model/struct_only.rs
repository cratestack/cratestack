//! Plain model `struct` emission (server + client variants) plus the
//! shared `struct_field_definition` field-token builder used by every
//! struct + input emitter.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::shared::{
    doc_attrs, ident, is_primary_key, is_server_only_field, rust_type_tokens,
    rust_type_tokens_with_scope, scalar_model_fields,
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

pub(crate) fn generate_client_model_struct(
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

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #model_ident {
            #(#fields)*
        }
    }
}

pub(crate) fn struct_field_definition(
    field: &Field,
    wrap_for_patch: bool,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let docs = doc_attrs(&field.docs);
    let base_type = if enum_names.contains(field.ty.name.as_str()) {
        let enum_ident = ident(&field.ty.name);
        match field.ty.arity {
            TypeArity::Required => quote! { super::types::#enum_ident },
            TypeArity::Optional => quote! { Option<super::types::#enum_ident> },
            TypeArity::List => quote! { Vec<super::types::#enum_ident> },
        }
    } else {
        rust_type_tokens_with_scope(&field.ty, true)
    };
    let field_type = if wrap_for_patch {
        quote! { Option<#base_type> }
    } else {
        base_type
    };
    // `@server_only` fields stay readable inside server code (SQLx populates
    // them via FromRow, which doesn't go through serde) but are masked from
    // both outbound JSON and inbound deserialization. The default value is
    // used if a client somehow sends one — banks shouldn't rely on that;
    // it's a defence-in-depth seam.
    let serde_attr = if is_server_only_field(field) {
        quote! { #[serde(skip_serializing, default)] }
    } else if matches!(field.ty.arity, TypeArity::Optional) && !wrap_for_patch {
        // Generated model structs declare Optional fields as `Option<T>`,
        // but the wire projection strips `null` map entries before the
        // codec sees them (CBOR/minicbor-serde encodes `Value::Null` as an
        // empty array, which would corrupt round-trips). `#[serde(default)]`
        // lets the client struct accept "missing field" as `None`,
        // restoring the round-trip without changing the wire format.
        quote! { #[serde(default)] }
    } else {
        quote! {}
    };

    quote! {
        #docs
        #serde_attr
        pub #field_ident: #field_type,
    }
}

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity, TypeDecl};
use quote::quote;

use crate::event::model_emitted_events;
use crate::policy::{
    generate_denies_for_action, generate_denies_for_actions, generate_policies_for_action,
    generate_policies_for_actions,
};
use crate::relation::{collect_allowed_sort_keys, generate_relation_order_module};
use crate::shared::{
    auth_default_field, create_sql_value, doc_attrs, generated_doc_attr, ident,
    is_generated_on_create, is_pii_field, is_primary_key, is_readonly_field, is_sensitive_field,
    is_server_only_field, is_version_field, model_name_set, pluralize, relation_model_fields,
    rust_type_tokens, rust_type_tokens_with_scope, scalar_model_fields, to_snake_case,
    update_sql_value,
};
use crate::validators::generate_input_validate_body;

mod selection;

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

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #model_ident {
            #(#fields)*
        }
    }
}

/// Emit `impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for Model` only.
/// **Server-side composer use only — must not appear in embedded output.**
pub(crate) fn generate_pg_from_row_impl(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let scalar_fields = scalar_model_fields(model, model_names);
    let row_fields = scalar_fields
        .iter()
        .map(|field| row_field_tokens(field, enum_names));

    quote! {
        impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for #model_ident {
            fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
                use sqlx::Row;

                Ok(Self {
                    #(#row_fields)*
                })
            }
        }
    }
}

/// Emit `impl FromRusqliteRow for Model` only.
/// **Embedded-side composer use only — must not appear in server output.**
pub(crate) fn generate_rusqlite_from_row_impl(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let scalar_fields = scalar_model_fields(model, model_names);
    let sqlite_row_fields = scalar_fields
        .iter()
        .map(|field| sqlite_row_field_tokens(field, enum_names));

    quote! {
        impl ::cratestack_rusqlite::FromRusqliteRow for #model_ident {
            fn from_rusqlite_row(
                row: &::cratestack_rusqlite::rusqlite::Row<'_>,
            ) -> ::cratestack_rusqlite::rusqlite::Result<Self> {
                Ok(Self {
                    #(#sqlite_row_fields)*
                })
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

pub(crate) fn generate_create_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Create{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated create input for `{}`.", model.name));
    let fields: Vec<_> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_generated_on_create(field))
        .filter(|field| !is_readonly_field(field))
        .filter(|field| !is_server_only_field(field))
        .filter(|field| !is_version_field(field))
        .collect();
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));
    let sql_values = fields
        .iter()
        .map(|field| create_sql_value(field, enum_names));
    let model_ident = ident(&model.name);
    let field_refs: Vec<&Field> = fields.iter().copied().collect();
    let validate_impl = match generate_input_validate_body(&field_refs, false) {
        Some(body) => quote! {
            fn validate(&self) -> ::std::result::Result<(), ::cratestack::CoolError> {
                #body
            }
        },
        None => quote! {},
    };

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #input_ident {
            #(#definitions)*
        }

        impl ::cratestack::CreateModelInput<super::models::#model_ident> for #input_ident {
            fn sql_values(&self) -> Vec<::cratestack::SqlColumnValue> {
                vec![#(#sql_values),*]
            }
            #validate_impl
        }
    }
}

pub(crate) fn generate_client_create_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Create{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated create input for `{}`.", model.name));
    let fields: Vec<_> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_generated_on_create(field))
        .filter(|field| !is_readonly_field(field))
        .filter(|field| !is_server_only_field(field))
        .filter(|field| !is_version_field(field))
        .collect();
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #input_ident {
            #(#definitions)*
        }
    }
}

pub(crate) fn generate_update_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Update{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated update input for `{}`.", model.name));
    let fields: Vec<_> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_primary_key(field))
        .filter(|field| !is_readonly_field(field))
        .filter(|field| !is_server_only_field(field))
        .filter(|field| !is_version_field(field))
        .collect();
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, true, enum_names));
    let sql_values = fields
        .iter()
        .map(|field| update_sql_value(field, enum_names));
    let model_ident = ident(&model.name);
    let field_refs: Vec<&Field> = fields.iter().copied().collect();
    let validate_impl = match generate_input_validate_body(&field_refs, true) {
        Some(body) => quote! {
            fn validate(&self) -> ::std::result::Result<(), ::cratestack::CoolError> {
                #body
            }
        },
        None => quote! {},
    };

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
        pub struct #input_ident {
            #(#definitions)*
        }

        impl ::cratestack::UpdateModelInput<super::models::#model_ident> for #input_ident {
            fn sql_values(&self) -> Vec<::cratestack::SqlColumnValue> {
                let mut values = Vec::new();
                #(#sql_values)*
                values
            }
            #validate_impl
        }
    }
}

pub(crate) fn generate_client_update_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Update{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated update input for `{}`.", model.name));
    let fields: Vec<_> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_primary_key(field))
        .filter(|field| !is_readonly_field(field))
        .filter(|field| !is_server_only_field(field))
        .filter(|field| !is_version_field(field))
        .collect();
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, true, enum_names));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
        pub struct #input_ident {
            #(#definitions)*
        }
    }
}

fn row_field_tokens(field: &Field, enum_names: &BTreeSet<&str>) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let field_name = &field.name;
    if !enum_names.contains(field.ty.name.as_str()) {
        return quote! {
            #field_ident: row.try_get(#field_name)?,
        };
    }

    let enum_ident = ident(&field.ty.name);
    let parse_error = |error: proc_macro2::TokenStream| {
        quote! {
            sqlx::Error::Decode(Box::new(::std::io::Error::new(
                ::std::io::ErrorKind::InvalidData,
                #error,
            )))
        }
    };

    match field.ty.arity {
        TypeArity::Required => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: String = row.try_get(#field_name)?;
                    raw.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)?
                },
            }
        }
        TypeArity::Optional => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: Option<String> = row.try_get(#field_name)?;
                    raw.map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)).transpose()?
                },
            }
        }
        TypeArity::List => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: Vec<String> = row.try_get(#field_name)?;
                    raw.into_iter()
                        .map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error))
                        .collect::<Result<Vec<_>, sqlx::Error>>()?
                },
            }
        }
    }
}

fn sqlite_row_field_tokens(
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let field_name = &field.name;

    // Enums round-trip as TEXT (same as the PG side).
    if enum_names.contains(field.ty.name.as_str()) {
        let enum_ident = ident(&field.ty.name);
        let parse_error = |error: proc_macro2::TokenStream| {
            quote! {
                ::cratestack::rusqlite::Error::FromSqlConversionFailure(
                    0,
                    ::cratestack::rusqlite::types::Type::Text,
                    Box::new(::std::io::Error::new(
                        ::std::io::ErrorKind::InvalidData,
                        #error,
                    )),
                )
            }
        };
        return match field.ty.arity {
            TypeArity::Required => {
                let decode_error = parse_error(quote! { error.to_string() });
                quote! {
                    #field_ident: {
                        let raw: String = row.get(#field_name)?;
                        raw.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)?
                    },
                }
            }
            TypeArity::Optional => {
                let decode_error = parse_error(quote! { error.to_string() });
                quote! {
                    #field_ident: {
                        let raw: Option<String> = row.get(#field_name)?;
                        raw.map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)).transpose()?
                    },
                }
            }
            TypeArity::List => {
                // SQLite has no native array storage; lists encode as JSON arrays
                // of variant strings. Decode-side mirrors that.
                let decode_error = parse_error(quote! { error.to_string() });
                quote! {
                    #field_ident: {
                        let raw: String = row.get(#field_name)?;
                        let strs: Vec<String> = ::serde_json::from_str(&raw)
                            .map_err(|error| #decode_error)?;
                        strs.into_iter()
                            .map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error))
                            .collect::<Result<Vec<_>, _>>()?
                    },
                }
            }
        };
    }

    // Scalar types: every type that's stored as TEXT on the device needs a
    // column-wrapper newtype so rusqlite's FromSql picks the right decoder.
    // Simple types (String, Int, Float, Bytes) use rusqlite's built-in FromSql
    // impls directly via `row.get(name)?`.
    match (field.ty.name.as_str(), field.ty.arity) {
        ("Boolean", TypeArity::Required) => quote! {
            #field_ident: row.get::<_, i64>(#field_name)? != 0,
        },
        ("Boolean", TypeArity::Optional) => quote! {
            #field_ident: row
                .get::<_, Option<i64>>(#field_name)?
                .map(|value| value != 0),
        },
        ("Uuid", TypeArity::Required) => quote! {
            #field_ident: row
                .get::<_, ::cratestack::UuidColumn>(#field_name)?
                .0,
        },
        ("Uuid", TypeArity::Optional) => quote! {
            #field_ident: row
                .get::<_, Option<::cratestack::UuidColumn>>(#field_name)?
                .map(|v| v.0),
        },
        ("DateTime", TypeArity::Required) => quote! {
            #field_ident: row
                .get::<_, ::cratestack::DateTimeColumn>(#field_name)?
                .0,
        },
        ("DateTime", TypeArity::Optional) => quote! {
            #field_ident: row
                .get::<_, Option<::cratestack::DateTimeColumn>>(#field_name)?
                .map(|v| v.0),
        },
        ("Decimal", TypeArity::Required) => quote! {
            #field_ident: row
                .get::<_, ::cratestack::DecimalColumn>(#field_name)?
                .0,
        },
        ("Decimal", TypeArity::Optional) => quote! {
            #field_ident: row
                .get::<_, Option<::cratestack::DecimalColumn>>(#field_name)?
                .map(|v| v.0),
        },
        ("Json", TypeArity::Required) => quote! {
            #field_ident: {
                let raw: String = row.get(#field_name)?;
                let value: ::cratestack::Value = ::serde_json::from_str(&raw)
                    .map_err(|error| ::cratestack::rusqlite::Error::FromSqlConversionFailure(
                        0,
                        ::cratestack::rusqlite::types::Type::Text,
                        Box::new(error),
                    ))?;
                ::cratestack::sqlx::types::Json(value)
            },
        },
        ("Json", TypeArity::Optional) => quote! {
            #field_ident: {
                let raw: Option<String> = row.get(#field_name)?;
                match raw {
                    Some(text) => {
                        let value: ::cratestack::Value = ::serde_json::from_str(&text)
                            .map_err(|error| ::cratestack::rusqlite::Error::FromSqlConversionFailure(
                                0,
                                ::cratestack::rusqlite::types::Type::Text,
                                Box::new(error),
                            ))?;
                        Some(::cratestack::sqlx::types::Json(value))
                    }
                    None => None,
                }
            },
        },
        // Default: rusqlite's built-in FromSql handles the conversion (String,
        // Int as i64, Float as f64, Bytes as Vec<u8>, Cuid as String).
        _ => quote! {
            #field_ident: row.get(#field_name)?,
        },
    }
}

fn struct_field_definition(
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

pub(crate) fn generate_model_descriptor(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    let model_ident = ident(&model.name);
    let descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&model.name).to_uppercase()
    ));
    let table_name = pluralize(&to_snake_case(&model.name));
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);
    let primary_key_sql = to_snake_case(&primary_key.name);
    let read_policies =
        generate_policies_for_actions(model, models, types, auth, &["list", "read"])?;
    let detail_policies =
        generate_policies_for_actions(model, models, types, auth, &["detail", "read"])?;
    let create_policies = generate_policies_for_action(model, models, types, auth, "create")?;
    let create_deny_policies = generate_denies_for_action(model, models, types, auth, "create")?;
    let update_policies = generate_policies_for_action(model, models, types, auth, "update")?;
    let update_deny_policies = generate_denies_for_action(model, models, types, auth, "update")?;
    let delete_policies = generate_policies_for_action(model, models, types, auth, "delete")?;
    let delete_deny_policies = generate_denies_for_action(model, models, types, auth, "delete")?;
    let read_deny_policies =
        generate_denies_for_actions(model, models, types, auth, &["list", "read"])?;
    let detail_deny_policies =
        generate_denies_for_actions(model, models, types, auth, &["detail", "read"])?;
    let create_defaults = scalar_model_fields(model, &model_name_set(models))
        .into_iter()
        .filter_map(|field| {
            let auth_field = auth_default_field(field)?;
            let column = to_snake_case(&field.name);
            let auth_field_decl = crate::policy::find_auth_field(auth, types, auth_field).map_err(|_| {
                    format!(
                        "auth-derived default on `{}.{}` references unknown auth field `{}`",
                        model.name, field.name, auth_field
                    )
                });
            let kind = match field.ty.name.as_str() {
                "String" | "Cuid" => Ok(quote! { ::cratestack::CreateDefaultType::String }),
                "Int" => Ok(quote! { ::cratestack::CreateDefaultType::Int }),
                "Boolean" => Ok(quote! { ::cratestack::CreateDefaultType::Bool }),
                other => Err(format!(
                    "auth-derived defaults currently support only String/Cuid, Int, and Boolean fields; `{}`.{} is unsupported",
                    model.name, other
                )),
            };
            let nullable = matches!(field.ty.arity, TypeArity::Optional);
            Some(auth_field_decl.and_then(|auth_field_decl| {
                if auth_field_decl.ty.name != field.ty.name && !(field.ty.name == "Cuid" && auth_field_decl.ty.name == "String") {
                    return Err(format!(
                        "auth-derived default on `{}.{}` requires matching auth/model field types",
                        model.name, field.name
                    ));
                }

                kind.map(|kind| quote! {
                ::cratestack::CreateDefault {
                    column: #column,
                    auth_field: #auth_field,
                    ty: #kind,
                    nullable: #nullable,
                }
                })
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let emitted_events = model_emitted_events(model)?
        .into_iter()
        .map(|operation| match operation {
            cratestack_core::ModelEventKind::Created => {
                quote! { ::cratestack::ModelEventKind::Created }
            }
            cratestack_core::ModelEventKind::Updated => {
                quote! { ::cratestack::ModelEventKind::Updated }
            }
            cratestack_core::ModelEventKind::Deleted => {
                quote! { ::cratestack::ModelEventKind::Deleted }
            }
        })
        .collect::<Vec<_>>();
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
        });
    let allowed_fields = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter(|field| !is_server_only_field(field))
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect::<Vec<_>>();
    let allowed_includes = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect::<Vec<_>>();
    let allowed_sorts = collect_allowed_sort_keys(model, models)?
        .into_iter()
        .map(|field| quote! { #field })
        .collect::<Vec<_>>();

    let version_column_tokens = match version_field(model) {
        Some(field) => {
            let column = to_snake_case(&field.name);
            quote! { Some(#column) }
        }
        None => quote! { None },
    };
    let audit_enabled = model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@audit");
    let pii_columns = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter(|field| is_pii_field(field))
        .map(|field| {
            let column = to_snake_case(&field.name);
            quote! { #column }
        })
        .collect::<Vec<_>>();
    let sensitive_columns = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter(|field| is_sensitive_field(field))
        .map(|field| {
            let column = to_snake_case(&field.name);
            quote! { #column }
        })
        .collect::<Vec<_>>();
    let soft_delete_enabled = model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@soft_delete");
    let soft_delete_column_tokens = if soft_delete_enabled {
        quote! { Some("deleted_at") }
    } else {
        quote! { None }
    };
    let retention_days_tokens = model
        .attributes
        .iter()
        .find_map(|attribute| {
            attribute
                .raw
                .strip_prefix("@@retain(days:")
                .and_then(|rest| rest.strip_suffix(')'))
                .map(str::trim)
                .and_then(|raw| raw.parse::<u32>().ok())
        })
        .map(|n| quote! { Some(#n) })
        .unwrap_or_else(|| quote! { None });

    Ok(quote! {
        pub const #descriptor_ident: ::cratestack::ModelDescriptor<#model_ident, #primary_key_type> =
            ::cratestack::ModelDescriptor::new(
                stringify!(#model_ident),
                #table_name,
                &[#(#columns),*],
                #primary_key_sql,
                &[#(#allowed_fields),*],
                &[#(#allowed_includes),*],
                &[#(#allowed_sorts),*],
                &[#(#read_policies),*],
                &[#(#read_deny_policies),*],
                &[#(#detail_policies),*],
                &[#(#detail_deny_policies),*],
                &[#(#create_policies),*],
                &[#(#create_deny_policies),*],
                &[#(#update_policies),*],
                &[#(#update_deny_policies),*],
                &[#(#delete_policies),*],
                &[#(#delete_deny_policies),*],
                &[#(#create_defaults),*],
                &[#(#emitted_events),*],
                #version_column_tokens,
                #audit_enabled,
                &[#(#pii_columns),*],
                &[#(#sensitive_columns),*],
                #soft_delete_column_tokens,
                #retention_days_tokens,
            );
    })
}

fn version_field(model: &Model) -> Option<&Field> {
    model
        .fields
        .iter()
        .find(|field| field.attributes.iter().any(|a| a.raw == "@version"))
}

pub(crate) fn generate_field_module(
    model: &Model,
    model_names: &BTreeSet<&str>,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let field_fns = scalar_model_fields(model, model_names).into_iter().map(|field| {
        let function_ident = ident(&field.name);
        let field_type = rust_type_tokens(&field.ty);
        let column = to_snake_case(&field.name);

        quote! {
            #[allow(non_snake_case)]
            pub fn #function_ident() -> ::cratestack::FieldRef<super::models::#model_ident, #field_type> {
                ::cratestack::FieldRef::new(#column)
            }
        }
    });
    let relation_root_fns = relation_model_fields(model, model_names)
        .into_iter()
        .map(|field| {
            let function_ident = ident(&field.name);
            let module_ident = ident(&field.name);

            quote! {
                #[allow(non_snake_case)]
                pub fn #function_ident() -> self::#module_ident::Path {
                    self::#module_ident::Path
                }
            }
        });
    let relation_modules = relation_model_fields(model, model_names)
        .into_iter()
        .map(|relation_field| generate_relation_order_module(model, relation_field, models))
        .collect::<Result<Vec<_>, String>>()?;
    let selection_module = generate_selection_module(model, model_names, models)?;

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            #(#field_fns)*
            #(#relation_root_fns)*
            pub fn select() -> self::selection::Selection {
                self::selection::Selection::default()
            }

            pub fn include_selection() -> self::selection::IncludeSelection {
                self::selection::IncludeSelection::default()
            }

            #(#relation_modules)*
            #selection_module
        }
    })
}

pub(crate) fn generate_model_accessor(model: &Model) -> proc_macro2::TokenStream {
    let method_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&model.name).to_uppercase()
    ));
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    quote! {
        pub fn #method_ident(&self) -> ::cratestack::ModelDelegate<'_, models::#model_ident, #primary_key_type> {
            ::cratestack::ModelDelegate::new(&self.runtime, &models::#descriptor_ident)
        }
    }
}

pub(crate) fn generate_bound_model_accessor(model: &Model) -> proc_macro2::TokenStream {
    let method_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    quote! {
        pub fn #method_ident(&self) -> ::cratestack::ScopedModelDelegate<'_, models::#model_ident, #primary_key_type> {
            self.inner.#method_ident().bind(self.ctx.clone())
        }
    }
}

fn generate_selection_module(
    model: &Model,
    model_names: &BTreeSet<&str>,
    _models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let model_name = &model.name;
    let scalar_fields = scalar_model_fields(model, model_names);
    let selection_field_methods = selection::build_selection_field_methods(&scalar_fields);
    let include_selection_field_methods = selection_field_methods.clone();
    let selected_scalar_accessors =
        selection::build_selected_scalar_accessors(&scalar_fields, model_name);
    let included_scalar_accessors = selected_scalar_accessors.clone();

    let relation_entries =
        selection::build_selection_relation_entries(model, model_names, model_name)?;

    let include_methods = relation_entries
        .iter()
        .map(|entry| entry.include_methods.clone())
        .collect::<Vec<_>>();
    let include_fields = relation_entries
        .iter()
        .map(|entry| entry.include_field.clone())
        .collect::<Vec<_>>();
    let include_query_steps = relation_entries
        .iter()
        .map(|entry| entry.include_query_step.clone())
        .collect::<Vec<_>>();
    let include_accessors = relation_entries
        .into_iter()
        .map(|entry| entry.include_accessor)
        .collect::<Vec<_>>();

    Ok(quote! {
        pub mod selection {
            #[derive(Debug, Clone, Default)]
            pub struct Includes {
                #(#include_fields)*
            }

            #[derive(Debug, Clone, Default)]
            pub struct Selection {
                fields: Option<::std::collections::BTreeSet<&'static str>>,
                includes: Includes,
            }

            impl Selection {
                pub fn all_fields(mut self) -> Self {
                    self.fields = None;
                    self
                }

                #(#selection_field_methods)*
                #(#include_methods)*

                pub fn to_query(&self) -> ::cratestack::SelectionQuery {
                    let mut query = ::cratestack::SelectionQuery::default();
                    if let Some(fields) = &self.fields {
                        query.fields = fields.iter().map(|field| (*field).to_owned()).collect();
                    }
                    #(#include_query_steps)*
                    query
                }

                pub fn decode_one(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<Projected, ::cratestack::CoolError> {
                    Projected::from_value(value, self.clone())
                }

                pub fn decode_many(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<Vec<Projected>, ::cratestack::CoolError> {
                    match value {
                        ::cratestack::serde_json::Value::Array(values) => values
                            .into_iter()
                            .map(|value| self.decode_one(value))
                            .collect(),
                        other => Err(::cratestack::CoolError::Internal(format!(
                            "projected {} list payload must be an array, got {other:?}",
                            #model_name,
                        ))),
                    }
                }

                pub fn decode_page(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<::cratestack::Page<Projected>, ::cratestack::CoolError> {
                    let page = ::cratestack::serde_json::from_value::<::cratestack::Page<::cratestack::serde_json::Value>>(value)
                        .map_err(|error| ::cratestack::CoolError::Codec(format!(
                            "failed to decode projected {} page payload: {error}",
                            #model_name,
                        )))?;
                    let items = page
                        .items
                        .into_iter()
                        .map(|value| self.decode_one(value))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(::cratestack::Page::new(items, page.page_info).with_total_count(page.total_count))
                }
            }

            impl ::cratestack::client_rust::Projection for Selection {
                type Output = Projected;

                fn selection_query(&self) -> ::cratestack::SelectionQuery {
                    self.to_query()
                }

                fn decode_one(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<Self::Output, ::cratestack::CoolError> {
                    Selection::decode_one(self, value)
                }

                fn decode_many(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<Vec<Self::Output>, ::cratestack::CoolError> {
                    Selection::decode_many(self, value)
                }

                fn decode_page(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<::cratestack::Page<Self::Output>, ::cratestack::CoolError> {
                    Selection::decode_page(self, value)
                }
            }

            #[derive(Debug, Clone, Default)]
            pub struct IncludeSelection {
                fields: Option<::std::collections::BTreeSet<&'static str>>,
                includes: Includes,
            }

            impl IncludeSelection {
                pub fn all_fields(mut self) -> Self {
                    self.fields = None;
                    self
                }

                #(#include_selection_field_methods)*
                #(#include_methods)*

                pub fn to_query(&self) -> ::cratestack::SelectionQuery {
                    let mut query = ::cratestack::SelectionQuery::default();
                    if let Some(fields) = &self.fields {
                        query.fields = fields.iter().map(|field| (*field).to_owned()).collect();
                    }
                    #(#include_query_steps)*
                    query
                }
            }

            #[derive(Debug, Clone)]
            pub struct Projected {
                fields: ::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
                selection: Selection,
            }

            impl Projected {
                fn from_value(
                    value: ::cratestack::serde_json::Value,
                    selection: Selection,
                ) -> Result<Self, ::cratestack::CoolError> {
                    match value {
                        ::cratestack::serde_json::Value::Object(fields) => Ok(Self { fields, selection }),
                        other => Err(::cratestack::CoolError::Internal(format!(
                            "projected {} payload must be an object, got {other:?}",
                            #model_name,
                        ))),
                    }
                }

                fn allows_field(&self, field: &str) -> bool {
                    match &self.selection.fields {
                        Some(fields) => fields.contains(field),
                        None => true,
                    }
                }

                pub fn raw(&self) -> &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value> {
                    &self.fields
                }

                #(#selected_scalar_accessors)*
                #(#include_accessors)*
            }

            #[derive(Debug, Clone)]
            pub struct ProjectedInclude {
                fields: ::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
                selection: IncludeSelection,
            }

            impl ProjectedInclude {
                pub(crate) fn from_value(
                    value: ::cratestack::serde_json::Value,
                    selection: IncludeSelection,
                ) -> Result<Self, ::cratestack::CoolError> {
                    match value {
                        ::cratestack::serde_json::Value::Object(fields) => Ok(Self { fields, selection }),
                        other => Err(::cratestack::CoolError::Internal(format!(
                            "projected included {} payload must be an object, got {other:?}",
                            #model_name,
                        ))),
                    }
                }

                fn allows_field(&self, field: &str) -> bool {
                    match &self.selection.fields {
                        Some(fields) => fields.contains(field),
                        None => true,
                    }
                }

                pub fn raw(&self) -> &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value> {
                    &self.fields
                }

                #(#included_scalar_accessors)*
                #(#include_accessors)*
            }

            fn decode_projected_field<T>(
                object: &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
                selected: bool,
                model_name: &str,
                field_name: &str,
            ) -> Result<T, ::cratestack::CoolError>
            where
                T: ::cratestack::serde::de::DeserializeOwned,
            {
                if !selected {
                    return Err(::cratestack::CoolError::Validation(format!(
                        "field '{}.{}' was not selected",
                        model_name,
                        field_name,
                    )));
                }

                let value = object.get(field_name).cloned().ok_or_else(|| {
                    ::cratestack::CoolError::Internal(format!(
                        "projected {} payload is missing field '{}'",
                        model_name,
                        field_name,
                    ))
                })?;

                ::cratestack::serde_json::from_value(value).map_err(|error| {
                    ::cratestack::CoolError::Internal(format!(
                        "failed to decode projected field '{}.{}': {error}",
                        model_name,
                        field_name,
                    ))
                })
            }
        }
    })
}

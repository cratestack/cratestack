use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, Model, TypeArity, TypeRef};
use quote::quote;
use syn::LitStr;

pub(crate) fn schema_lit(value: &str) -> LitStr {
    LitStr::new(value, proc_macro2::Span::call_site())
}

pub(crate) fn ident(value: &str) -> syn::Ident {
    syn::Ident::new(value, proc_macro2::Span::call_site())
}

pub(crate) fn doc_attrs(docs: &[String]) -> proc_macro2::TokenStream {
    let attrs = docs.iter().map(|doc| {
        quote! {
            #[doc = #doc]
        }
    });
    quote! {
        #(#attrs)*
    }
}

pub(crate) fn generated_doc_attr(doc: impl AsRef<str>) -> proc_macro2::TokenStream {
    let doc = doc.as_ref();
    quote! {
        #[doc = #doc]
    }
}

pub(crate) fn supports_comparison(field: &Field) -> bool {
    field.ty.arity == TypeArity::Required
        && matches!(
            field.ty.name.as_str(),
            "String" | "Cuid" | "Int" | "Float" | "DateTime" | "Decimal" | "Uuid"
        )
}

pub(crate) fn model_name_set(models: &[Model]) -> BTreeSet<&str> {
    models.iter().map(|model| model.name.as_str()).collect()
}

pub(crate) fn enum_name_set(enums: &[EnumDecl]) -> BTreeSet<&str> {
    enums
        .iter()
        .map(|enum_decl| enum_decl.name.as_str())
        .collect()
}

pub(crate) fn scalar_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_relation_field(model_names, field))
        .collect()
}

pub(crate) fn is_custom_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@custom")
}

pub(crate) fn relation_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| is_relation_field(model_names, field))
        .collect()
}

pub(crate) fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

pub(crate) fn find_model<'a>(models: &'a [Model], name: &str) -> Option<&'a Model> {
    models.iter().find(|model| model.name == name)
}

pub(crate) fn is_primary_key(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

pub(crate) fn is_paged_model(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@paged")
}

/// Field carries `@readonly` — must not be settable via Create or Update
/// inputs. Server code can still write the column directly via SQL.
pub(crate) fn is_readonly_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@readonly")
}

/// Field carries `@server_only` — never accepted on input AND stripped
/// from client-facing responses. Use for fields like internal scoring,
/// risk flags, or hashed secrets that the server owns end-to-end.
pub(crate) fn is_server_only_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}

/// Field carries `@pii` — personally identifiable. Values are redacted in
/// the audit log JSON and (in a follow-up) in tracing/error detail.
pub(crate) fn is_pii_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@pii")
}

/// Field carries `@sensitive` — application-defined sensitive data that
/// doesn't fit PII (internal risk scores, dispute notes). Redacted in the
/// audit log JSON.
pub(crate) fn is_sensitive_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@sensitive")
}

fn has_default(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@default"))
}

pub(crate) fn auth_default_field(field: &Field) -> Option<&str> {
    field.attributes.iter().find_map(|attribute| {
        let inner = attribute
            .raw
            .trim()
            .strip_prefix("@default(")?
            .strip_suffix(')')?
            .trim();
        inner.strip_prefix("auth().").map(str::trim)
    })
}

pub(crate) fn is_generated_on_create(field: &Field) -> bool {
    has_default(field)
}

/// Field carries `@version` — the optimistic-lock column. The SQL builder
/// emits `version = version + 1` on every update, and seeds the column on
/// create, so it must never appear in Create or Update input structs.
/// Letting it through would either let clients seed the initial version
/// (create) or produce duplicate `version = ...` SQL assignments (update).
pub(crate) fn is_version_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@version")
}

pub(crate) fn query_scalar_parser_tokens(
    ty: &TypeRef,
    value_expr: proc_macro2::TokenStream,
    field_name: &str,
) -> Option<proc_macro2::TokenStream> {
    Some(match ty.name.as_str() {
        "String" => quote! { Ok((#value_expr).to_owned()) },
        "Cuid" => quote! { ::cratestack::parse_cuid(#value_expr) },
        "Int" => quote! {
            (#value_expr).parse::<i64>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Float" => quote! {
            (#value_expr).parse::<f64>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Boolean" => quote! {
            (#value_expr).parse::<bool>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Uuid" => quote! {
            (#value_expr).parse::<::cratestack::uuid::Uuid>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "DateTime" => quote! {
            (#value_expr)
                .parse::<::cratestack::chrono::DateTime<::cratestack::chrono::FixedOffset>>()
                .map(|value| value.with_timezone(&::cratestack::chrono::Utc))
                .map_err(|error| {
                    CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
                })
        },
        "Decimal" => quote! {
            (#value_expr).parse::<::cratestack::Decimal>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        _ => return None,
    })
}

pub(crate) fn query_scalar_list_parser_tokens(
    ty: &TypeRef,
    field_name: &str,
) -> Option<proc_macro2::TokenStream> {
    let scalar_parser = query_scalar_parser_tokens(ty, quote! { raw_value }, field_name)?;

    Some(quote! {{
        let parsed = value
            .split(',')
            .map(str::trim)
            .filter(|raw_value| !raw_value.is_empty())
            .map(|raw_value| -> Result<_, CoolError> { #scalar_parser })
            .collect::<Result<Vec<_>, CoolError>>()?;
        if parsed.is_empty() {
            return Err(CoolError::BadRequest(format!(
                "{}__in requires at least one value",
                #field_name,
            )));
        }
        parsed
    }})
}

pub(crate) fn rust_type_tokens(type_ref: &TypeRef) -> proc_macro2::TokenStream {
    rust_type_tokens_with_scope(type_ref, true)
}

pub(crate) fn rust_type_tokens_with_scope(
    type_ref: &TypeRef,
    custom_in_super: bool,
) -> proc_macro2::TokenStream {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_type = rust_type_tokens_with_scope(item, custom_in_super);
        return quote! { ::cratestack::Page<#item_type> };
    }

    let inner = match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Decimal" => quote! { ::cratestack::Decimal },
        "Json" => quote! { ::cratestack::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        other => {
            let ident = ident(other);
            if custom_in_super {
                quote! { super::#ident }
            } else {
                quote! { #ident }
            }
        }
    };

    match type_ref.arity {
        TypeArity::Required => inner,
        TypeArity::Optional => quote! { Option<#inner> },
        TypeArity::List => quote! { Vec<#inner> },
    }
}

pub(crate) fn field_definition(
    field: &Field,
    wrap_for_patch: bool,
    custom_in_super: bool,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let docs = doc_attrs(&field.docs);
    let base_type = rust_type_tokens_with_scope(&field.ty, custom_in_super);
    let field_type = if wrap_for_patch {
        quote! { Option<#base_type> }
    } else {
        base_type
    };

    quote! {
        #docs
        pub #field_ident: #field_type,
    }
}

pub(crate) fn create_sql_value(
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let column = to_snake_case(&field.name);
    let value = sql_value_tokens(quote! { self.#field_ident.clone() }, &field.ty, enum_names);

    quote! {
        ::cratestack::SqlColumnValue {
            column: #column,
            value: #value,
        }
    }
}

pub(crate) fn update_sql_value(
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let column = to_snake_case(&field.name);
    let some_value = sql_value_tokens(quote! { value }, &field.ty, enum_names);

    quote! {
        if let Some(value) = self.#field_ident.clone() {
            values.push(::cratestack::SqlColumnValue {
                column: #column,
                value: #some_value,
            });
        }
    }
}

pub(crate) fn sql_value_tokens(
    value: proc_macro2::TokenStream,
    ty: &TypeRef,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    if enum_names.contains(ty.name.as_str()) {
        return match ty.arity {
            TypeArity::Required => quote! { ::cratestack::SqlValue::String(#value.to_string()) },
            TypeArity::Optional => quote! {
                match #value {
                    Some(value) => ::cratestack::SqlValue::String(value.to_string()),
                    None => ::cratestack::SqlValue::NullString,
                }
            },
            TypeArity::List => panic!("unsupported SQLx enum list type for this slice"),
        };
    }

    match (ty.name.as_str(), ty.arity) {
        ("String", TypeArity::Required) => quote! { ::cratestack::SqlValue::String(#value) },
        ("Cuid", TypeArity::Required) => quote! { ::cratestack::SqlValue::String(#value) },
        ("String", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::String(value),
                None => ::cratestack::SqlValue::NullString,
            }
        },
        ("Cuid", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::String(value),
                None => ::cratestack::SqlValue::NullString,
            }
        },
        ("Int", TypeArity::Required) => quote! { ::cratestack::SqlValue::Int(#value) },
        ("Int", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Int(value),
                None => ::cratestack::SqlValue::NullInt,
            }
        },
        ("Float", TypeArity::Required) => quote! { ::cratestack::SqlValue::Float(#value) },
        ("Float", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Float(value),
                None => ::cratestack::SqlValue::NullFloat,
            }
        },
        ("Boolean", TypeArity::Required) => quote! { ::cratestack::SqlValue::Bool(#value) },
        ("Boolean", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Bool(value),
                None => ::cratestack::SqlValue::NullBool,
            }
        },
        ("Bytes", TypeArity::Required) => quote! { ::cratestack::SqlValue::Bytes(#value) },
        ("Bytes", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Bytes(value),
                None => ::cratestack::SqlValue::NullBytes,
            }
        },
        ("Uuid", TypeArity::Required) => quote! { ::cratestack::SqlValue::Uuid(#value) },
        ("Uuid", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Uuid(value),
                None => ::cratestack::SqlValue::NullUuid,
            }
        },
        ("DateTime", TypeArity::Required) => quote! { ::cratestack::SqlValue::DateTime(#value) },
        ("DateTime", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::DateTime(value),
                None => ::cratestack::SqlValue::NullDateTime,
            }
        },
        ("Json", TypeArity::Required) => quote! { ::cratestack::SqlValue::Json(#value.0) },
        ("Json", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Json(value.0),
                None => ::cratestack::SqlValue::NullJson,
            }
        },
        ("Decimal", TypeArity::Required) => quote! { ::cratestack::SqlValue::Decimal(#value) },
        ("Decimal", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Decimal(value),
                None => ::cratestack::SqlValue::NullDecimal,
            }
        },
        _ => panic!("unsupported SQLx value type for this slice"),
    }
}

pub(crate) fn value_tokens(
    value: proc_macro2::TokenStream,
    ty: &TypeRef,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    if enum_names.contains(ty.name.as_str()) {
        return match ty.arity {
            TypeArity::Required => quote! { ::cratestack::Value::String(#value.to_string()) },
            TypeArity::Optional => quote! {
                match #value {
                    Some(value) => ::cratestack::Value::String(value.to_string()),
                    None => ::cratestack::Value::Null,
                }
            },
            TypeArity::List => quote! {
                ::cratestack::Value::List(
                    #value
                        .into_iter()
                        .map(|value| ::cratestack::Value::String(value.to_string()))
                        .collect()
                )
            },
        };
    }

    match (ty.name.as_str(), ty.arity) {
        ("String", TypeArity::Required) => quote! { ::cratestack::Value::String(#value) },
        ("Cuid", TypeArity::Required) => quote! { ::cratestack::Value::String(#value) },
        ("String", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::String(value),
                None => ::cratestack::Value::Null,
            }
        },
        ("Cuid", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::String(value),
                None => ::cratestack::Value::Null,
            }
        },
        ("Int", TypeArity::Required) => quote! { ::cratestack::Value::Int(#value) },
        ("Int", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::Int(value),
                None => ::cratestack::Value::Null,
            }
        },
        ("Boolean", TypeArity::Required) => quote! { ::cratestack::Value::Bool(#value) },
        ("Boolean", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::Bool(value),
                None => ::cratestack::Value::Null,
            }
        },
        _ => quote! { ::cratestack::Value::Null },
    }
}

pub(crate) fn to_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                output.push('_');
            }
            for lowercase in character.to_lowercase() {
                output.push(lowercase);
            }
        } else {
            output.push(character);
        }
    }
    output
}

pub(crate) fn pluralize(value: &str) -> String {
    if value.ends_with('s') {
        format!("{value}es")
    } else {
        format!("{value}s")
    }
}

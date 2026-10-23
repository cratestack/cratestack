//! `generated_defaults` builder: one `GeneratedDefault` per field whose
//! `@default(...)` is `now()`, `uuid()` or `cuid()` (cratestack#1209). The
//! runtime fills these on create when the input leaves them out, so the
//! value never depends on a database `DEFAULT`.

use cratestack_core::Model;
use cratestack_core::schema::default_functions::{BuiltinDefault, DefaultValue, classify_default};
use cratestack_core::schema::default_markers::default_inner;
use quote::quote;

use crate::shared::{model_name_set, scalar_model_fields, to_snake_case};

pub(super) fn collect_generated_defaults(
    model: &Model,
    models: &[Model],
) -> Vec<proc_macro2::TokenStream> {
    scalar_model_fields(model, &model_name_set(models))
        .into_iter()
        .filter_map(|field| {
            // The parser has already refused every other spelling, an
            // argument, and a field type the value does not fit.
            let DefaultValue::Builtin(builtin, _) = classify_default(default_inner(field)?) else {
                return None;
            };
            let value = match builtin {
                BuiltinDefault::Now => quote! { ::cratestack::GeneratedValue::Now },
                BuiltinDefault::Uuid if field.ty.name == "Uuid" => {
                    quote! { ::cratestack::GeneratedValue::Uuid }
                }
                BuiltinDefault::Uuid => quote! { ::cratestack::GeneratedValue::UuidText },
                BuiltinDefault::Cuid => quote! { ::cratestack::GeneratedValue::Cuid },
            };
            let column = to_snake_case(&field.name);
            Some(quote! {
                ::cratestack::GeneratedDefault { column: #column, value: #value }
            })
        })
        .collect()
}

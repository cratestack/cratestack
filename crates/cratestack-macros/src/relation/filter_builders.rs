//! Per-field filter token emitters used by relation filter codegen.
//! Two parallel slices: [`methods`] (chainable `self`-consumers) and
//! [`functions`] (free-standing helper `fn`s). Both rely on
//! [`op_expr`] to wrap a `FieldRef<>::<op>` call in the outer
//! `FilterExpr::from(...)` plus any nested-relation wrappers.

use quote::quote;

use super::{RelationPathSegment, wrap_filter_expr_tokens};

mod functions;
mod methods;

pub(super) use functions::{
    append_boolean_filter_functions, append_optional_filter_functions,
    append_optional_string_filter_functions, append_required_filter_functions,
    append_required_text_filter_functions,
};
pub(super) use methods::{
    append_boolean_builder_methods, append_optional_builder_methods,
    append_optional_string_builder_methods, append_required_builder_methods,
    append_required_text_builder_methods,
};

fn op_expr(
    field_type: &proc_macro2::TokenStream,
    column: &str,
    op_call: proc_macro2::TokenStream,
    wrappers: &[RelationPathSegment],
) -> proc_macro2::TokenStream {
    wrap_filter_expr_tokens(
        quote! {
            ::cratestack::FilterExpr::from(
                ::cratestack::FieldRef::<(), #field_type>::new(#column).#op_call
            )
        },
        wrappers,
    )
}

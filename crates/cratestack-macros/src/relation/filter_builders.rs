//! Per-field filter method emitters used by relation filter codegen.
//!
//! All emitted methods are `self`-consuming builders, so they fold the
//! runtime `self.hops` path into the resulting `FilterExpr` via
//! [`cratestack_sql::wrap_filter`]. The previous free-function variants
//! (`post::author::profile::nickname_eq(..)`) were removed: addressing a
//! relation path by *module path* is precisely what forced one emitted
//! module per distinct path, i.e. the exponential blowup in
//! cratestack#252. The chained form
//! (`post::author().profile().nickname().eq(..)`) is the 1:1 replacement.

use quote::quote;

mod methods;

pub(crate) use methods::{
    append_boolean_builder_methods, append_optional_builder_methods,
    append_optional_string_builder_methods, append_required_builder_methods,
    append_required_text_builder_methods,
};

fn op_expr(
    field_type: &proc_macro2::TokenStream,
    column: &str,
    op_call: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::wrap_filter(
            &self.hops,
            ::cratestack::FilterExpr::from(
                ::cratestack::FieldRef::<(), #field_type>::new(#column).#op_call
            ),
        )
    }
}

//! Projection leaves (cratestack#1425). A `ProjectedValue::leaf` serializes
//! the raw field value and so bypasses the generated struct's `Serialize` —
//! and with it the `serialize_with` on the field. A byte-valued leaf is
//! therefore wrapped in `::cratestack::WireBytes`, which writes the same
//! byte string under CBOR and the unchanged array under JSON (the leaf runs
//! against the real serializer through erased-serde, which forwards
//! `is_human_readable`).

use cratestack_core::TypeRef;
use quote::quote;

/// `::cratestack::ProjectedValue::leaf(<expr>)`, with `<expr>` wrapped in
/// `WireBytes` when `ty` is `Bytes`/`Geography`/`Geometry` (any arity).
pub(crate) fn projected_leaf_tokens(
    ty: &TypeRef,
    expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if matches!(ty.name.as_str(), "Bytes" | "Geography" | "Geometry") {
        quote! { ::cratestack::ProjectedValue::leaf(::cratestack::WireBytes(#expr)) }
    } else {
        quote! { ::cratestack::ProjectedValue::leaf(#expr) }
    }
}

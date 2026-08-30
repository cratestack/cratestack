//! Picks the `#[serde(deserialize_with = "…")]` a generated `Bytes` field
//! needs so it accepts a CBOR byte string (RFC 8949 major type 2) as well
//! as the integer array every deployed client sends — cratestack#783.
//!
//! `Bytes` generates as `Vec<u8>`, whose blanket `Deserialize` accepts a
//! sequence and nothing else, so a `Uint8Array` encoded the way every
//! other CBOR producer encodes binary data is rejected at the codec before
//! the handler is ever reached. `cratestack_core::lenient_bytes` holds the
//! deserializers that accept both; this module is the mapping from a
//! field's generated *shape* to the right one, because
//! `#[serde(deserialize_with)]` names a concrete function whose return
//! type has to match the field type exactly.
//!
//! The shape table below must stay in lockstep with
//! [`super::types::field_type`] — same `(wrap_for_patch, arity)` inputs,
//! same resulting Rust type:
//!
//! | `wrap_for_patch` | arity      | field type                |
//! |------------------|------------|---------------------------|
//! | `false`          | `Required` | `Vec<u8>`                 |
//! | `false`          | `Optional` | `Option<Vec<u8>>`         |
//! | `false`          | `List`     | `Vec<Vec<u8>>`            |
//! | `true`           | `Required` | `Option<Vec<u8>>`         |
//! | `true`           | `Optional` | `Option<Option<Vec<u8>>>` |
//! | `true`           | `List`     | `Option<Vec<Vec<u8>>>`    |
//!
//! Inbound only — nothing here changes what a generated `Serialize`
//! emits. See `cratestack_core::lenient_bytes`'s module doc for why the
//! outbound shape is deliberately left alone.

use cratestack_core::{TypeArity, TypeRef};
use quote::quote;

/// What a `Bytes`-typed field's `#[serde(...)]` list needs, split into two
/// pieces because the emitters differ in what they already have: the
/// model/CRUD-input emitter's branches carry their own `default`, while
/// the `type`-block and procedure-`Args` emitters carry no serde
/// attributes at all.
pub(crate) struct BytesSerde {
    /// The `deserialize_with = "…"` argument itself.
    pub(crate) deserialize_with: proc_macro2::TokenStream,
    /// Whether `default` must accompany it. True exactly when the field
    /// type is an `Option<_>`: a custom `deserialize_with` opts a field
    /// out of serde-derive's implicit "missing `Option<T>` field defaults
    /// to `None`" handling (the trap [`cratestack_core::patch`]
    /// documents), so without it an omitted nullable `Bytes` field would
    /// become a hard decode error — a regression, not a fix.
    pub(crate) needs_default: bool,
}

impl BytesSerde {
    /// The full argument list, `default` first, for emitters starting from
    /// an empty `#[serde(...)]`.
    pub(crate) fn args(&self) -> Vec<proc_macro2::TokenStream> {
        let deserialize_with = &self.deserialize_with;
        if self.needs_default {
            vec![quote! { default }, quote! { #deserialize_with }]
        } else {
            vec![quote! { #deserialize_with }]
        }
    }
}

/// `None` for every type but the byte-valued ones. `Page<Bytes>`/
/// `FindMany<Bytes>` fall out here too — their `TypeRef::name` is the
/// wrapper, not `Bytes`.
///
/// `Geography`/`Geometry` opt in alongside `Bytes` (cratestack#842):
/// their Rust type is `Vec<u8>` holding EWKB, so they need exactly the
/// same base64 JSON treatment. Leaving them out would serialize a
/// geometry as a JSON array of integers on the REST/RPC surface while
/// every other byte-valued field is base64.
pub(crate) fn bytes_serde(ty: &TypeRef, wrap_for_patch: bool) -> Option<BytesSerde> {
    if !matches!(ty.name.as_str(), "Bytes" | "Geography" | "Geometry") {
        return None;
    }

    let (helper, needs_default) = match (wrap_for_patch, ty.arity) {
        (false, TypeArity::Required) => ("deserialize_bytes", false),
        (false, TypeArity::Optional) => ("deserialize_optional_bytes", true),
        (false, TypeArity::List) => ("deserialize_bytes_list", false),
        (true, TypeArity::Required) => ("deserialize_optional_bytes", true),
        (true, TypeArity::Optional) => ("deserialize_double_option_bytes", true),
        (true, TypeArity::List) => ("deserialize_optional_bytes_list", true),
    };

    let path = format!("::cratestack::{helper}");
    Some(BytesSerde {
        deserialize_with: quote! { deserialize_with = #path },
        needs_default,
    })
}

/// The whole `#[serde(...)]` attribute, for the emitters that put no
/// serde attributes on a field otherwise — `type` blocks (both the server
/// and client copies), their `wire` mirror, and procedure `Args`. Expands
/// to nothing for every type but `Bytes`.
pub(crate) fn bytes_serde_attr(ty: &TypeRef, wrap_for_patch: bool) -> proc_macro2::TokenStream {
    match bytes_serde(ty, wrap_for_patch) {
        Some(bytes) => {
            let args = bytes.args();
            quote! { #[serde(#(#args),*)] }
        }
        None => quote! {},
    }
}

/// The `deserialize_with` argument alone, for a field whose emitter
/// already contributes its own `default` — the model/CRUD-input path.
/// The caller keeps its existing `default`; adding a second one would be
/// a duplicate-attribute compile error, which is why this exists
/// alongside [`bytes_serde_attr`] rather than being folded into it.
pub(crate) fn bytes_deserialize_with(
    ty: &TypeRef,
    wrap_for_patch: bool,
) -> Option<proc_macro2::TokenStream> {
    bytes_serde(ty, wrap_for_patch).map(|bytes| bytes.deserialize_with)
}

#[cfg(test)]
mod tests;

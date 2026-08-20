//! [`BuilderField`] — one field of the struct a typestate builder is being
//! generated for. Split from `builder.rs` per the repo's 200-LoC file
//! convention; the *why* of the encoding lives in that module's doc.

use quote::quote;

/// One field of the struct being built.
pub(crate) struct BuilderField {
    /// Field identifier, already raw-escaped by [`crate::shared::ident`].
    pub(crate) ident: syn::Ident,
    /// The field's emitted type, verbatim — the same tokens the struct
    /// definition uses.
    pub(crate) ty: proc_macro2::TokenStream,
    /// What the setter accepts. Same as [`Self::ty`] everywhere except
    /// update inputs, where it is `ty` with one `Option` layer peeled:
    /// on a patch struct the outer `Option` *is* "did the caller touch
    /// this field", which calling the setter answers by itself. So
    /// `.name("Ops")` sets a non-nullable column and `.email(None)`
    /// clears a nullable one — instead of both needing an extra `Some(..)`
    /// that could only ever be written one way.
    pub(crate) setter_ty: proc_macro2::TokenStream,
    /// Whether the stored value needs that peeled `Option` put back.
    patch: bool,
    /// `false` only for `Option<T>` / `Vec<T>` fields; see the module doc.
    pub(crate) required: bool,
    /// Take `impl Into<T>` instead of `T`. Set for plain `String` fields
    /// so `.name("Ops")` works; deliberately *not* set for numeric types,
    /// where the extra inference variable turns unannotated integer
    /// literals into fallback-dependent guesswork.
    pub(crate) into: bool,
    /// The element type of a list-arity field's `.add_{field}(item)`
    /// setter — `Some` iff this field is a list. Derived by the field-spec
    /// builder from the very same type-token function that produced
    /// [`Self::ty`] (asking it what a `Required`-arity clone of the field
    /// would type as — arity is the only input that turns `T` into
    /// `Vec<T>`), so the element type can never drift from the field type.
    pub(crate) elem_ty: Option<proc_macro2::TokenStream>,
    /// `impl Into<Elem>` for the append setter — same rule as [`Self::into`]:
    /// `String`/`Cuid` elements only.
    pub(crate) elem_into: bool,
    /// The append setter's name, `add_{field}`, mechanically derived (no
    /// singularization — see `docs` on the schema-level collision this
    /// implies). `Some` iff [`Self::elem_ty`] is `Some`.
    pub(crate) append_ident: Option<syn::Ident>,
    /// Doc attributes carried over from the schema, so the setter reads
    /// the same as the field.
    pub(crate) docs: proc_macro2::TokenStream,
}

impl BuilderField {
    pub(crate) fn new(
        ident: syn::Ident,
        ty: proc_macro2::TokenStream,
        required: bool,
    ) -> BuilderField {
        BuilderField {
            ident,
            setter_ty: ty.clone(),
            ty,
            patch: false,
            required,
            into: false,
            elem_ty: None,
            elem_into: false,
            append_ident: None,
            docs: quote! {},
        }
    }

    /// Mark this as an update-input field: `setter_ty` is the type
    /// *before* patch-wrapping, and the setter puts the outer `Option`
    /// back on the way in.
    pub(crate) fn with_patch(mut self, setter_ty: proc_macro2::TokenStream) -> BuilderField {
        self.setter_ty = setter_ty;
        self.patch = true;
        self
    }

    pub(crate) fn with_into(mut self, into: bool) -> BuilderField {
        self.into = into;
        self
    }

    /// Mark this as a list field with an append setter alongside the bulk
    /// one. `elem_ty` and `append_ident` come from the same type-token /
    /// naming machinery the field type and field ident already went
    /// through — see the field docs above.
    pub(crate) fn with_list(
        mut self,
        elem_ty: proc_macro2::TokenStream,
        elem_into: bool,
        append_ident: syn::Ident,
    ) -> BuilderField {
        self.elem_ty = Some(elem_ty);
        self.elem_into = elem_into;
        self.append_ident = Some(append_ident);
        self
    }

    /// The expression a setter stores into the holder: one `Some` for the
    /// holder's own "was this setter called" layer, plus a second one for
    /// a patch field's peeled `Option`.
    pub(crate) fn store_expr(&self, value: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
        if self.patch {
            quote! { ::core::option::Option::Some(::core::option::Option::Some(#value)) }
        } else {
            quote! { ::core::option::Option::Some(#value) }
        }
    }

    /// The statement an append setter uses to push one element into the
    /// holder. Mirrors [`Self::store_expr`]'s two shapes (plain vs.
    /// patch-wrapped), but mutates in place via `get_or_insert_with`
    /// rather than replacing the slot outright — `.add_{field}` must
    /// preserve whatever the bulk setter (or a prior append) already put
    /// there, and on a patch struct must also flip the outer "touched"
    /// `Option` to `Some` without disturbing an inner value it finds
    /// already set.
    pub(crate) fn append_stmt(
        &self,
        tuple_slot: &syn::Index,
        value: proc_macro2::TokenStream,
    ) -> proc_macro2::TokenStream {
        if self.patch {
            quote! {
                self.fields.#tuple_slot
                    .get_or_insert_with(|| ::core::option::Option::Some(::std::vec::Vec::new()))
                    .get_or_insert_with(::std::vec::Vec::new)
                    .push(#value);
            }
        } else {
            quote! {
                self.fields.#tuple_slot
                    .get_or_insert_with(::std::vec::Vec::new)
                    .push(#value);
            }
        }
    }

    pub(crate) fn with_docs(mut self, docs: proc_macro2::TokenStream) -> BuilderField {
        self.docs = docs;
        self
    }
}

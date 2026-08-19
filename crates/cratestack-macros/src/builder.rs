//! Typestate builder emission, shared by every struct-shaped type the
//! three entry macros generate (model structs, CRUD inputs, `{Model}Where`,
//! `{Model}OrderByClause`, `{Model}FindManyInput`, `view` structs, `type`
//! structs, per-procedure `Args`) on both the server and client sides.
//!
//! The generated shape and *why* required-field enforcement is a compile
//! error rather than a `Result` is documented once, on the marker types:
//! see `cratestack_core::builder`.
//!
//! ## Why the state parameters cover only the *required* fields
//!
//! The obvious encoding — one type-level slot per field — makes a 30-field
//! model emit a 30-parameter generic that every setter signature repeats.
//! Optional fields never gate `build()`, so they get no slot: their setters
//! return `Self` and the state tuple stays as short as the number of fields
//! a caller can actually forget. An all-optional struct (every `Where`,
//! every `Update{Model}Input`, `{Model}FindManyInput`) therefore ends up
//! with a plain non-generic builder and no `PhantomData` churn at all.
//!
//! ## Why the values live in one anonymous tuple
//!
//! A setter has to move the builder from one *type* to another, so it can't
//! use struct-update syntax (`..self`) — the source and target types differ.
//! Storing every value as its own builder field would force each setter to
//! re-list every field, which is quadratic in emitted tokens. Holding them
//! all in a single `fields` member means a setter moves exactly one value
//! regardless of field count.
//!
//! That member is an anonymous tuple rather than a named
//! `{Type}BuilderFields` struct specifically so it claims **no name** in the
//! generated module. A named holder would be a third generated identifier
//! per struct that a schema could collide with — and unlike `{Type}Builder`,
//! which is public API a caller has to be able to name, nothing outside the
//! setters ever refers to this one. Cheaper to make the collision
//! impossible than to validate against it. (`self.fields.3` reads worse in
//! `cargo expand` output than `self.fields.email` would; that is the whole
//! of the cost.)
//!
//! ## What counts as "optional"
//!
//! Exactly the two emitted shapes whose `Default` *is* the correct "the
//! caller said nothing" value: `Option<T>` (absent / NULL) and `Vec<T>`
//! (empty list). Everything else is required. Callers pass this in rather
//! than having it re-derived here, because the caller is the only place
//! that knows which of `TypeArity`, patch-wrapping, or a `Page<T>` /
//! `FindMany<T>` special case produced the type tokens it also passes in.

mod emit;
mod fields;
mod state;

use quote::{format_ident, quote};

pub(crate) use fields::{model_builder_fields, scoped_builder_fields};
pub(crate) use state::StateParams;

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

    pub(crate) fn with_docs(mut self, docs: proc_macro2::TokenStream) -> BuilderField {
        self.docs = docs;
        self
    }
}

/// Emit `impl {Target} { fn builder() }`, the `{Target}Builder` typestate
/// struct, its private field holder, every setter, and the `build()` impl
/// that only exists in the fully-set state.
///
/// The `{Target}Builder` type is emitted into the same module as
/// `{Target}` — every call site already emits the struct definition there.
pub(crate) fn generate_builder(
    target: &syn::Ident,
    fields: &[BuilderField],
) -> proc_macro2::TokenStream {
    let builder_ident = format_ident!("{}Builder", target);
    let state = StateParams::new(fields);

    let holder_ty = fields.iter().map(|field| {
        let field_ty = &field.ty;
        quote! { ::core::option::Option<#field_ty>, }
    });
    // `(None, None, ..)` written out rather than `Default::default()`:
    // std implements `Default` for tuples only up to arity 12, and a
    // 13-field model is entirely ordinary.
    let holder_init = fields
        .iter()
        .map(|_| quote! { ::core::option::Option::None, });

    let decl_generics = state.declaration();
    let impl_generics = state.impl_params();
    let self_ty = state.applied(&builder_ident);
    let phantom_ty = state.phantom_ty();
    let setters = fields
        .iter()
        .enumerate()
        .map(|(index, field)| emit::setter(index, field, &builder_ident, &state));
    let build_impl = emit::build_impl(target, &builder_ident, &state, fields);

    let builder_docs = format!(
        "Typestate builder for [`{target}`] — `build()` only exists once every required field \
         has been set. Start one with `{target}::builder()`."
    );

    quote! {
        impl #target {
            #[doc = #builder_docs]
            pub fn builder() -> #builder_ident {
                #builder_ident {
                    fields: (#(#holder_init)*),
                    __state: ::core::marker::PhantomData,
                }
            }
        }

        #[doc = #builder_docs]
        pub struct #builder_ident #decl_generics {
            fields: (#(#holder_ty)*),
            __state: ::core::marker::PhantomData<#phantom_ty>,
        }

        impl #impl_generics #self_ty {
            #(#setters)*
        }

        #build_impl
    }
}

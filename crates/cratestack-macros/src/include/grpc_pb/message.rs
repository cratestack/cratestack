//! Generic single-presence-level message renderer — used for `Model`,
//! `type` (`TypeDecl`), and `Create<M>Input` mirrors (every field on those
//! three maps 1:1 onto a plain proto3 `optional`/`repeated` field, same as
//! `cratestack-proto::emit::message::render_message`'s default case).
//! `Update<M>Input`'s patch semantics need an extra `Option` layer the wire
//! can't express, so it gets its own renderer (`create_update`).
//!
//! Emits, per message: the `#[derive(::cratestack::grpc::prost::Message)]`
//! mirror struct, `impl From<&Domain> for Mirror` (always infallible — every
//! domain scalar has a lossless wire representation on the way out), and
//! `impl TryFrom<Mirror> for Domain` (fallible — parsing untrusted wire
//! bytes can fail: bad UUID/decimal/JSON/timestamp text, an unknown enum
//! discriminant, a missing required field).

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, TypeArity, TypeRef};
use quote::quote;

use crate::shared::ident;

use super::scalar::{domain_from_wire_expr, scalar_wire, wire_from_domain_expr};

pub(crate) struct RenderedMessage {
    pub(crate) tokens: proc_macro2::TokenStream,
}

/// `message_name`: the pb struct's name (`User`, `CreateUserInput`, ...).
/// `domain_path`: tokens resolving to the existing domain struct
/// (`super::super::User`) — a sibling of the `pb` module, reachable via
/// `cratestack_schema`'s own `pub use models::*;` / `pub use inputs::*;`
/// glob re-exports.
pub(crate) fn render_message(
    message_name: &str,
    domain_path: proc_macro2::TokenStream,
    fields: &[&Field],
    numbers: &BTreeMap<String, i32>,
    enum_names: &BTreeSet<&str>,
) -> Result<RenderedMessage, String> {
    let ident_tok = ident(message_name);
    let mut prost_fields = Vec::with_capacity(fields.len());
    let mut from_domain_inits = Vec::with_capacity(fields.len());
    let mut try_from_wire_lets = Vec::with_capacity(fields.len());
    let mut try_from_wire_inits = Vec::with_capacity(fields.len());

    for field in fields {
        let number = *numbers
            .get(&field.name)
            .ok_or_else(|| format!("no `.pb.lock` entry for `{message_name}.{}`", field.name))?;
        let field_ident = ident(&field.name);
        let domain_expr = quote! { value.#field_ident.clone() };
        let plan = render_field(
            message_name,
            &field.name,
            &field.ty,
            domain_expr,
            number,
            enum_names,
        );
        prost_fields.push(plan.prost_field);
        from_domain_inits.push(plan.from_domain_init);
        try_from_wire_lets.push(plan.try_from_wire_let);
        try_from_wire_inits.push(quote! { #field_ident, });
    }

    let tokens = quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #(#prost_fields)*
        }

        impl ::core::convert::From<&#domain_path> for #ident_tok {
            fn from(value: &#domain_path) -> Self {
                Self {
                    #(#from_domain_inits)*
                }
            }
        }

        impl ::core::convert::TryFrom<#ident_tok> for #domain_path {
            type Error = ::cratestack::CratestackError;

            fn try_from(value: #ident_tok) -> ::core::result::Result<Self, Self::Error> {
                #(#try_from_wire_lets)*
                Ok(Self {
                    #(#try_from_wire_inits)*
                })
            }
        }
    };

    Ok(RenderedMessage { tokens })
}

pub(crate) struct FieldPlan {
    pub(crate) prost_field: proc_macro2::TokenStream,
    pub(crate) from_domain_init: proc_macro2::TokenStream,
    pub(crate) try_from_wire_let: proc_macro2::TokenStream,
}

fn missing_field_error(owner: &str, field: &str) -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::CratestackError::BadRequest(format!("missing required field {}.{}", #owner, #field))
    }
}

/// Builds one field's prost attribute plus both conversion directions.
/// `domain_expr` is the *owned* expression that yields the field's domain
/// value — ordinary message fields pass `value.<field>.clone()`
/// (`render_message`'s call site); [`super::super::server::grpc::
/// procedures::render_procedure_output`] (ticket #208) reuses this same
/// per-type/per-arity logic for a procedure's single synthetic `result`
/// field, passing `(*value).clone()` instead since there is no struct
/// field to project through — the domain value *is* `value` there.
/// `owner`/`field_name` only feed error messages and the wire ident, so
/// this generalizes without needing a real [`Field`] to exist.
pub(crate) fn render_field(
    owner: &str,
    field_name: &str,
    ty: &TypeRef,
    domain_expr: proc_macro2::TokenStream,
    number: i32,
    enum_names: &BTreeSet<&str>,
) -> FieldPlan {
    let field_ident = ident(field_name);
    let type_name = ty.name.as_str();
    let number_lit = proc_macro2::Literal::i32_unsuffixed(number);
    let missing = missing_field_error(owner, field_name);

    if let Some(wire) = scalar_wire(type_name) {
        let rust_inner = &wire.rust_type;
        let kind = &wire.prost_kind;
        let to_wire = |expr| wire_from_domain_expr(type_name, expr);
        let to_domain = |expr| domain_from_wire_expr(type_name, expr, owner, field_name);

        return match ty.arity {
            TypeArity::Required => {
                let wire_expr = to_wire(domain_expr);
                let domain_conv = to_domain(quote! { raw });
                FieldPlan {
                    prost_field: quote! {
                        #[prost(#kind, optional, tag = #number_lit)]
                        pub #field_ident: Option<#rust_inner>,
                    },
                    from_domain_init: quote! { #field_ident: Some(#wire_expr), },
                    try_from_wire_let: quote! {
                        let #field_ident = {
                            let raw = value.#field_ident.ok_or_else(|| #missing)?;
                            (#domain_conv)?
                        };
                    },
                }
            }
            TypeArity::Optional => {
                let wire_expr = to_wire(quote! { inner });
                let domain_conv = to_domain(quote! { raw });
                FieldPlan {
                    prost_field: quote! {
                        #[prost(#kind, optional, tag = #number_lit)]
                        pub #field_ident: Option<#rust_inner>,
                    },
                    from_domain_init: quote! {
                        #field_ident: (#domain_expr).map(|inner| #wire_expr),
                    },
                    try_from_wire_let: quote! {
                        let #field_ident = value.#field_ident
                            .map(|raw| -> ::core::result::Result<_, ::cratestack::CratestackError> { #domain_conv })
                            .transpose()?;
                    },
                }
            }
            TypeArity::List => {
                let wire_expr = to_wire(quote! { inner });
                let domain_conv = to_domain(quote! { raw });
                FieldPlan {
                    prost_field: quote! {
                        #[prost(#kind, repeated, tag = #number_lit)]
                        pub #field_ident: Vec<#rust_inner>,
                    },
                    from_domain_init: quote! {
                        #field_ident: (#domain_expr).into_iter().map(|inner| #wire_expr).collect(),
                    },
                    try_from_wire_let: quote! {
                        let #field_ident = value.#field_ident
                            .into_iter()
                            .map(|raw| -> ::core::result::Result<_, ::cratestack::CratestackError> { #domain_conv })
                            .collect::<::core::result::Result<Vec<_>, ::cratestack::CratestackError>>()?;
                    },
                }
            }
        };
    }

    if enum_names.contains(type_name) {
        // Represented as plain `int32` on the wire, not prost's
        // `enumeration = "..."` attribute: that attribute exists mainly to
        // generate convenience accessor methods requiring the named type to
        // implement `::prost::Enumeration` (i.e. a *second*, prost-specific
        // enum type mirroring the domain enum) — machinery this crate
        // doesn't need, since the `From`/`TryFrom` conversions below handle
        // the i32 <-> domain-enum mapping directly by hand.
        let enum_ident = ident(type_name);
        let domain_enum_path = quote! { super::super::#enum_ident };
        return match ty.arity {
            TypeArity::Required => FieldPlan {
                prost_field: quote! {
                    #[prost(int32, optional, tag = #number_lit)]
                    pub #field_ident: Option<i32>,
                },
                from_domain_init: quote! {
                    #field_ident: Some(i32::from(&(#domain_expr))),
                },
                try_from_wire_let: quote! {
                    let #field_ident = {
                        let raw = value.#field_ident.ok_or_else(|| #missing)?;
                        <#domain_enum_path as ::core::convert::TryFrom<i32>>::try_from(raw)?
                    };
                },
            },
            TypeArity::Optional => FieldPlan {
                prost_field: quote! {
                    #[prost(int32, optional, tag = #number_lit)]
                    pub #field_ident: Option<i32>,
                },
                from_domain_init: quote! {
                    #field_ident: (#domain_expr).map(|inner| i32::from(&inner)),
                },
                try_from_wire_let: quote! {
                    let #field_ident = value.#field_ident
                        .map(<#domain_enum_path as ::core::convert::TryFrom<i32>>::try_from)
                        .transpose()?;
                },
            },
            TypeArity::List => FieldPlan {
                prost_field: quote! {
                    #[prost(int32, repeated, tag = #number_lit)]
                    pub #field_ident: Vec<i32>,
                },
                from_domain_init: quote! {
                    #field_ident: (#domain_expr).iter().map(i32::from).collect(),
                },
                try_from_wire_let: quote! {
                    let #field_ident = value.#field_ident
                        .into_iter()
                        .map(<#domain_enum_path as ::core::convert::TryFrom<i32>>::try_from)
                        .collect::<::core::result::Result<Vec<_>, ::cratestack::CratestackError>>()?;
                },
            },
        };
    }

    // Message reference (a `model`/`type` mirror in this same `pb` module).
    // Boxed on non-list arities defensively, in case of self-/mutual
    // reference — `Vec<T>` is already heap-indirected, so list fields stay
    // unboxed.
    let message_ident = ident(type_name);
    let domain_message_path = quote! { super::super::#message_ident };
    match ty.arity {
        TypeArity::Required => FieldPlan {
            prost_field: quote! {
                #[prost(message, optional, boxed, tag = #number_lit)]
                pub #field_ident: Option<Box<#message_ident>>,
            },
            from_domain_init: quote! {
                #field_ident: Some(Box::new(#message_ident::from(&(#domain_expr)))),
            },
            try_from_wire_let: quote! {
                let #field_ident = {
                    let raw = value.#field_ident.ok_or_else(|| #missing)?;
                    #domain_message_path::try_from(*raw)?
                };
            },
        },
        TypeArity::Optional => FieldPlan {
            prost_field: quote! {
                #[prost(message, optional, boxed, tag = #number_lit)]
                pub #field_ident: Option<Box<#message_ident>>,
            },
            from_domain_init: quote! {
                #field_ident: (#domain_expr).map(|inner| Box::new(#message_ident::from(&inner))),
            },
            try_from_wire_let: quote! {
                let #field_ident = value.#field_ident
                    .map(|raw| #domain_message_path::try_from(*raw))
                    .transpose()?;
            },
        },
        TypeArity::List => FieldPlan {
            prost_field: quote! {
                #[prost(message, repeated, tag = #number_lit)]
                pub #field_ident: Vec<#message_ident>,
            },
            from_domain_init: quote! {
                #field_ident: (#domain_expr).iter().map(#message_ident::from).collect(),
            },
            try_from_wire_let: quote! {
                let #field_ident = value.#field_ident
                    .into_iter()
                    .map(#domain_message_path::try_from)
                    .collect::<::core::result::Result<Vec<_>, ::cratestack::CratestackError>>()?;
            },
        },
    }
}

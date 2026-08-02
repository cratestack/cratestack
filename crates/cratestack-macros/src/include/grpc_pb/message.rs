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

use cratestack_core::{Field, TypeArity};
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
        let plan = render_field(message_name, field, number, enum_names);
        prost_fields.push(plan.prost_field);
        from_domain_inits.push(plan.from_domain_init);
        try_from_wire_lets.push(plan.try_from_wire_let);
        let field_ident = ident(&field.name);
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
            type Error = ::cratestack::CoolError;

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

struct FieldPlan {
    prost_field: proc_macro2::TokenStream,
    from_domain_init: proc_macro2::TokenStream,
    try_from_wire_let: proc_macro2::TokenStream,
}

fn missing_field_error(owner: &str, field: &str) -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::CoolError::BadRequest(format!("missing required field {}.{}", #owner, #field))
    }
}

fn render_field(owner: &str, field: &Field, number: i32, enum_names: &BTreeSet<&str>) -> FieldPlan {
    let field_ident = ident(&field.name);
    let field_name = field.name.as_str();
    let type_name = field.ty.name.as_str();
    let number_lit = proc_macro2::Literal::i32_unsuffixed(number);
    let missing = missing_field_error(owner, field_name);
    let domain_expr = quote! { value.#field_ident.clone() };

    if let Some(wire) = scalar_wire(type_name) {
        let rust_inner = &wire.rust_type;
        let kind = &wire.prost_kind;
        let to_wire = |expr| wire_from_domain_expr(type_name, expr);
        let to_domain = |expr| domain_from_wire_expr(type_name, expr, owner, field_name);

        return match field.ty.arity {
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
                            .map(|raw| -> ::core::result::Result<_, ::cratestack::CoolError> { #domain_conv })
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
                            .map(|raw| -> ::core::result::Result<_, ::cratestack::CoolError> { #domain_conv })
                            .collect::<::core::result::Result<Vec<_>, ::cratestack::CoolError>>()?;
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
        return match field.ty.arity {
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
                        .collect::<::core::result::Result<Vec<_>, ::cratestack::CoolError>>()?;
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
    match field.ty.arity {
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
                    .collect::<::core::result::Result<Vec<_>, ::cratestack::CoolError>>()?;
            },
        },
    }
}

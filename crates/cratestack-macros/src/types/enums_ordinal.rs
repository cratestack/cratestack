//! The ordinal half of an enum's generated type (`Pending = 1`,
//! `docs/design/enum-ordinals.md` §2.2–§2.3): `#[repr(i16)]` discriminants,
//! an integer `Serialize`, and a `Deserialize` that takes the integer or the
//! variant's name. Names stay the type's `Display`/`FromStr`.
//!
//! Every enum, ordinal or not, also carries three hidden items so the row
//! decoders and SQL binders can be written once: `__CRATESTACK_ORDINAL`
//! picks the column's storage (a `const`, so the other arm is dead code),
//! `__cratestack_from_ordinal` turns a stored integer back into a variant,
//! and `__cratestack_sql_null` is the typed NULL its column takes.

use cratestack_core::EnumDecl;
use quote::quote;

use crate::shared::{ident, schema_lit};

/// `Self::A => 1,` per variant.
fn ordinal_arms(e: &EnumDecl) -> Vec<proc_macro2::TokenStream> {
    e.variants
        .iter()
        .filter_map(|v| {
            let variant = ident(&v.name);
            let ordinal = i16::try_from(v.ordinal?).ok()?;
            Some(quote! { #ordinal => Ok(Self::#variant), })
        })
        .collect()
}

fn accepted_ordinals(e: &EnumDecl) -> syn::LitStr {
    let list = e
        .variants
        .iter()
        .filter_map(|v| v.ordinal.map(|o| o.to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    schema_lit(&list)
}

/// The hidden storage items every server and embedded enum carries.
pub(super) fn storage_items(e: &EnumDecl) -> proc_macro2::TokenStream {
    let enum_name = schema_lit(&e.name);
    let (ordinal, null, from_ordinal) = if e.is_ordinal() {
        let arms = ordinal_arms(e);
        let accepted = accepted_ordinals(e);
        (
            quote! { true },
            quote! { ::cratestack::SqlValue::NullInt },
            quote! {
                match i16::try_from(value).ok() {
                    #(Some(#arms))*
                    _ => Err(format!(
                        "unknown ordinal {value} for `{}`; expected one of: {}",
                        #enum_name, #accepted,
                    )),
                }
            },
        )
    } else {
        (
            quote! { false },
            quote! { ::cratestack::SqlValue::NullString },
            quote! {
                Err(format!(
                    "`{}` is stored by name, not by ordinal; got {value}",
                    #enum_name,
                ))
            },
        )
    };
    quote! {
        #[doc(hidden)]
        pub const __CRATESTACK_ORDINAL: bool = #ordinal;

        #[doc(hidden)]
        pub fn __cratestack_from_ordinal(value: i64) -> ::core::result::Result<Self, String> {
            #from_ordinal
        }

        #[doc(hidden)]
        pub fn __cratestack_sql_null() -> ::cratestack::SqlValue {
            #null
        }
    }
}

/// `ordinal()` plus the integer serde impls, for an ordinal enum only.
pub(super) fn ordinal_impls(e: &EnumDecl) -> proc_macro2::TokenStream {
    let enum_ident = ident(&e.name);
    let enum_name = schema_lit(&e.name);
    let arms = ordinal_arms(e);
    let accepted = accepted_ordinals(e);
    quote! {
        impl #enum_ident {
            /// The number this variant is stored and sent as.
            pub const fn ordinal(self) -> i16 {
                self as i16
            }
        }

        impl ::core::convert::TryFrom<i16> for #enum_ident {
            type Error = String;

            fn try_from(value: i16) -> ::core::result::Result<Self, Self::Error> {
                match value {
                    #(#arms)*
                    _ => Err(format!(
                        "unknown ordinal {value} for `{}`; expected one of: {}",
                        #enum_name, #accepted,
                    )),
                }
            }
        }

        impl serde::Serialize for #enum_ident {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.serialize_i16(self.ordinal())
            }
        }

        // Tolerant decoding (§2.3): the integer, or the variant's name, so a
        // peer still sending names keeps working.
        impl<'de> serde::Deserialize<'de> for #enum_ident {
            fn deserialize<D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::core::result::Result<Self, D::Error> {
                struct OrdinalOrName;

                impl<'de> serde::de::Visitor<'de> for OrdinalOrName {
                    type Value = #enum_ident;

                    fn expecting(
                        &self,
                        formatter: &mut ::core::fmt::Formatter<'_>,
                    ) -> ::core::fmt::Result {
                        write!(formatter, "an ordinal or a variant name of `{}`", #enum_name)
                    }

                    fn visit_i64<E: serde::de::Error>(self, value: i64) -> ::core::result::Result<Self::Value, E> {
                        i16::try_from(value)
                            .map_err(|_| E::custom(format!("ordinal {value} is out of range")))
                            .and_then(|v| #enum_ident::try_from(v).map_err(E::custom))
                    }

                    fn visit_u64<E: serde::de::Error>(self, value: u64) -> ::core::result::Result<Self::Value, E> {
                        i64::try_from(value)
                            .map_err(|_| E::custom(format!("ordinal {value} is out of range")))
                            .and_then(|v| self.visit_i64(v))
                    }

                    fn visit_str<E: serde::de::Error>(self, value: &str) -> ::core::result::Result<Self::Value, E> {
                        value.parse::<#enum_ident>().map_err(E::custom)
                    }
                }

                deserializer.deserialize_any(OrdinalOrName)
            }
        }
    }
}

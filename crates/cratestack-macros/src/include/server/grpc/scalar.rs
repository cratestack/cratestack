//! Scalar type mapping for prost mirror-struct fields — the Rust-codegen
//! twin of `cratestack-proto::emit::scalar::map_scalar`
//! (`docs/design/protobuf.md` §4.1-4.3). That module maps a `.cstack`
//! scalar name to a *`.proto` type string*; this one maps the same name to
//! (a) the prost wire-representation Rust type and `#[prost(...)]`
//! attribute, and (b) the domain<->wire conversion expressions.
//!
//! Reimplemented locally rather than shared with `cratestack-proto`: the
//! two crates emit different target languages (proto text vs. Rust tokens)
//! from the same source table, and `cratestack-proto` must not depend on
//! `cratestack-macros` (crate-layering rule, see this repo's `CLAUDE.md`).

use quote::quote;

pub(super) struct ScalarWire {
    /// The prost field's Rust type, unwrapped (before `Option<_>`/`Vec<_>`
    /// arity wrapping).
    pub(super) rust_type: proc_macro2::TokenStream,
    /// The bare `#[prost(...)]` kind token(s), e.g. `string`, `int64`,
    /// `bytes = "vec"`, `message`.
    pub(super) prost_kind: proc_macro2::TokenStream,
}

/// `None` for a name this table doesn't know about — the caller resolves
/// it as either an enum or a message reference instead (mirrors
/// `cratestack-proto::emit::scalar::map_scalar`'s `other => plain(other)`
/// passthrough, except this table needs to distinguish "no mapping" from
/// "the literal string type" so the caller can pick the right dispatch).
pub(super) fn scalar_wire(name: &str) -> Option<ScalarWire> {
    Some(match name {
        "String" | "Cuid" | "Uuid" | "Decimal" => ScalarWire {
            rust_type: quote! { String },
            prost_kind: quote! { string },
        },
        "Int" => ScalarWire {
            rust_type: quote! { i64 },
            prost_kind: quote! { int64 },
        },
        "Float" => ScalarWire {
            rust_type: quote! { f64 },
            prost_kind: quote! { double },
        },
        "Boolean" => ScalarWire {
            rust_type: quote! { bool },
            prost_kind: quote! { bool },
        },
        "Bytes" | "Json" => ScalarWire {
            rust_type: quote! { Vec<u8> },
            prost_kind: quote! { bytes = "vec" },
        },
        "DateTime" => ScalarWire {
            rust_type: quote! { ::cratestack::grpc::prost_types::Timestamp },
            prost_kind: quote! { message },
        },
        _ => return None,
    })
}

/// Domain value (owned expression, e.g. `value.email.clone()`) -> wire
/// inner value expression. Always infallible — every non-trivial case
/// (`Uuid`/`Decimal`/`Json`/`DateTime`) has a lossless string/bytes/message
/// representation on the way *out*; only the way back in (parsing
/// untrusted wire bytes) can fail. See [`domain_from_wire_expr`].
pub(super) fn wire_from_domain_expr(
    scalar_name: &str,
    domain_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match scalar_name {
        "String" | "Cuid" | "Int" | "Float" | "Boolean" | "Bytes" => domain_expr,
        "Uuid" | "Decimal" => quote! { (#domain_expr).to_string() },
        "Json" => {
            quote! { ::cratestack::serde_json::to_vec(&(#domain_expr).0).unwrap_or_default() }
        }
        "DateTime" => quote! {
            ::cratestack::grpc::prost_types::Timestamp {
                seconds: (#domain_expr).timestamp(),
                nanos: (#domain_expr).timestamp_subsec_nanos() as i32,
            }
        },
        other => unreachable!("wire_from_domain_expr called with non-scalar name `{other}`"),
    }
}

/// Wire inner value expression -> `Result<DomainInner, ::cratestack::CoolError>`.
/// `owner`/`field` name the message/field this conversion belongs to, for
/// error messages.
pub(super) fn domain_from_wire_expr(
    scalar_name: &str,
    wire_expr: proc_macro2::TokenStream,
    owner: &str,
    field: &str,
) -> proc_macro2::TokenStream {
    let context = format!("{owner}.{field}");
    match scalar_name {
        "String" | "Cuid" | "Int" | "Float" | "Boolean" | "Bytes" => quote! { Ok(#wire_expr) },
        "Uuid" => quote! {
            (#wire_expr).parse::<::cratestack::uuid::Uuid>().map_err(|error| {
                ::cratestack::CoolError::BadRequest(format!("invalid uuid for {}: {error}", #context))
            })
        },
        "Decimal" => quote! {
            (#wire_expr).parse::<::cratestack::Decimal>().map_err(|error| {
                ::cratestack::CoolError::BadRequest(format!("invalid decimal for {}: {error}", #context))
            })
        },
        "Json" => quote! {
            ::cratestack::serde_json::from_slice::<::cratestack::Value>(&(#wire_expr))
                .map(::cratestack::Json)
                .map_err(|error| {
                    ::cratestack::CoolError::BadRequest(format!("invalid json for {}: {error}", #context))
                })
        },
        "DateTime" => quote! {
            ::cratestack::chrono::DateTime::<::cratestack::chrono::Utc>::from_timestamp(
                (#wire_expr).seconds,
                u32::try_from((#wire_expr).nanos).unwrap_or_default(),
            )
            .ok_or_else(|| ::cratestack::CoolError::BadRequest(format!("invalid timestamp for {}", #context)))
        },
        other => unreachable!("domain_from_wire_expr called with non-scalar name `{other}`"),
    }
}

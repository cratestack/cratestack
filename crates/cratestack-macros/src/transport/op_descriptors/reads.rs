//! The `model.<X>.list` and `model.<X>.get` descriptors, split out so the
//! RPC `OPS` table ([`super::generate_model_op_descriptors`]) and the MCP
//! resource table (`include::server::mcp_module::resources_table`,
//! cratestack#1040) are built by the same function. A resource read is
//! admitted by L3 with exactly the flags an RPC `list`/`get` would carry,
//! so the two surfaces cannot disagree about whether a read is rate
//! limited (cratestack#474's lesson, as for tools).

use cratestack_core::Model;
use quote::quote;

/// Reads are safe to retry without a key (`idempotent_by_default`), and
/// model CRUD has no `@no_rate_limit` opt-out, so every read is rate
/// limited (docs/design/extensions.md §5).
const IDEMPOTENT: bool = true;
const RATE_LIMITED: bool = true;

pub(crate) fn model_list_op_descriptor(
    model: &Model,
    auth_required: bool,
) -> proc_macro2::TokenStream {
    let model_name = model.name.as_str();
    super::op_descriptor(
        &format!("model.{model_name}.list"),
        quote! { ::cratestack::OpKind::Unary },
        "",
        &format!("Page<{model_name}>"),
        IDEMPOTENT,
        RATE_LIMITED,
        auth_required,
    )
}

pub(crate) fn model_get_op_descriptor(
    model: &Model,
    auth_required: bool,
) -> proc_macro2::TokenStream {
    let model_name = model.name.as_str();
    super::op_descriptor(
        &format!("model.{model_name}.get"),
        quote! { ::cratestack::OpKind::Unary },
        "",
        model_name,
        IDEMPOTENT,
        RATE_LIMITED,
        auth_required,
    )
}

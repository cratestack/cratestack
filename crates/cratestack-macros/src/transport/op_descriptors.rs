//! RPC op descriptors emitted into the generated `OPS` const.
//!
//! See `docs/design/rpc-transport.md` for the semantic spec.
//! `auth_required` is currently a placeholder — set to `true` whenever
//! the schema declares an `auth` block, `false` otherwise. Per-op
//! policy resolution is future work.

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_idempotency;

use cratestack_core::{Model, Procedure, TypeArity};
use quote::quote;

pub(crate) fn generate_model_op_descriptors(
    model: &Model,
    auth_required: bool,
) -> Vec<proc_macro2::TokenStream> {
    let model_name = model.name.as_str();
    let page_ty = format!("Page<{model_name}>");
    let create_input = format!("Create{model_name}Input");
    let update_input = format!("Update{model_name}Input");

    let list_id = format!("model.{model_name}.list");
    let get_id = format!("model.{model_name}.get");
    let create_id = format!("model.{model_name}.create");
    let update_id = format!("model.{model_name}.update");
    let delete_id = format!("model.{model_name}.delete");

    // Model CRUD ops have no `@no_rate_limit`-equivalent opt-out today
    // (that attribute is procedure-only, per docs/design/extensions.md §5),
    // so every one of them always participates in rate limiting.
    let rate_limited = true;

    // cratestack#743: a suppressed verb (`@@internal(...)`) advertises
    // no `OpDescriptor` at all — nothing tells an RPC client the op is
    // callable, matching REST's omitted route (design doc §3, RPC
    // unary row). `model_internal_actions` is the one shared source of
    // truth every surface consults; this is this surface's single call
    // site.
    let internal = cratestack_core::model_internal_actions(model);

    let mut descriptors = Vec::new();
    if !internal.contains("list") {
        descriptors.push(op_descriptor(
            &list_id,
            quote! { ::cratestack::OpKind::Unary },
            "",
            &page_ty,
            true,
            rate_limited,
            auth_required,
        ));
    }
    if !internal.contains("get") {
        descriptors.push(op_descriptor(
            &get_id,
            quote! { ::cratestack::OpKind::Unary },
            "",
            model_name,
            true,
            rate_limited,
            auth_required,
        ));
    }
    if !internal.contains("create") {
        descriptors.push(op_descriptor(
            &create_id,
            quote! { ::cratestack::OpKind::Unary },
            &create_input,
            model_name,
            false,
            rate_limited,
            auth_required,
        ));
    }
    if !internal.contains("update") {
        descriptors.push(op_descriptor(
            &update_id,
            quote! { ::cratestack::OpKind::Unary },
            &update_input,
            model_name,
            false,
            rate_limited,
            auth_required,
        ));
    }
    if !internal.contains("delete") {
        descriptors.push(op_descriptor(
            &delete_id,
            quote! { ::cratestack::OpKind::Unary },
            "",
            model_name,
            false,
            rate_limited,
            auth_required,
        ));
    }
    descriptors
}

/// `model.<X>.subscribe` op descriptor — only for models declaring
/// `@@subscribe` (the parser guarantees such a model also declares
/// `@@emit(...)`, and only under `transport rpc`, see
/// `cratestack-parser::validate::model_attributes`). Mirrors the shape
/// of the CRUD descriptors above but with `OpKind::Subscription`, no
/// input, and an output type naming the streamed envelope rather than
/// the bare model — see `docs/design/rpc-transport.md` §3.4a.
pub(crate) fn generate_model_subscribe_op_descriptor(
    model: &Model,
    auth_required: bool,
) -> Option<proc_macro2::TokenStream> {
    if !model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@subscribe")
    {
        return None;
    }
    let model_name = model.name.as_str();
    let op_id = format!("model.{model_name}.subscribe");
    let output_ty = format!("ModelEvent<{model_name}>");
    Some(op_descriptor(
        &op_id,
        quote! { ::cratestack::OpKind::Subscription },
        "",
        &output_ty,
        // No idempotency key concept applies to a GET stream; reads are
        // inherently safe to reconnect, mirroring the CRUD `get`/`list`
        // descriptors above.
        true,
        true,
        auth_required,
    ))
}

pub(crate) fn generate_procedure_op_descriptor(
    procedure: &Procedure,
    auth_required: bool,
) -> proc_macro2::TokenStream {
    let op_id = format!("procedure.{}", procedure.name);
    let kind = if matches!(procedure.return_type.arity, TypeArity::List) {
        quote! { ::cratestack::OpKind::Sequence }
    } else {
        quote! { ::cratestack::OpKind::Unary }
    };
    // For now, the input type is the first arg's type name (the
    // conventional single-`args` arg). Procedures with zero or
    // multiple args expose an empty `input_ty`; richer surfacing is
    // future work.
    let input_ty = procedure
        .args
        .first()
        .map(|a| a.ty.name.as_str())
        .unwrap_or("");
    let output_ty = procedure.return_type.name.as_str();
    // Queries are safe to retry without an idempotency key; mutations are
    // not, unless the author opted out with `@no_idempotency`. Shared with
    // `transport::rest` so the two transports cannot answer differently.
    let idempotent = super::idempotency::procedure_idempotent_by_default(procedure);
    let rate_limited = super::rate_limit::procedure_rate_limited_by_default(procedure);

    op_descriptor(
        &op_id,
        kind,
        input_ty,
        output_ty,
        idempotent,
        rate_limited,
        auth_required,
    )
}

#[allow(clippy::too_many_arguments)]
fn op_descriptor(
    op_id: &str,
    kind: proc_macro2::TokenStream,
    input_ty: &str,
    output_ty: &str,
    idempotent: bool,
    rate_limited: bool,
    auth_required: bool,
) -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::OpDescriptor {
            op_id: #op_id,
            kind: #kind,
            input_ty: #input_ty,
            output_ty: #output_ty,
            idempotent_by_default: #idempotent,
            rate_limited_by_default: #rate_limited,
            auth_required: #auth_required,
        }
    }
}

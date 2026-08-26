//! Orchestrates `model.<X>.{list,get,create,update,delete}` RPC
//! dispatch-arm emission: builds the shared idents/ids/paths once into
//! [`ModelRpcContext`], then asks [`arms_read`]/[`arms_write`] for each
//! verb's `quote!` arm — skipping any verb `@@internal(...)` suppresses
//! (cratestack#743, `docs/design/route-suppression.md`).
//!
//! cratestack#743: a suppressed verb
//! (`cratestack_core::model_internal_actions` — the one shared source
//! of truth this surface consults) gets no arm at all, so
//! `rpc_dispatch_inner`'s `match op_id` falls through to its
//! pre-existing `other => ...` catch-all, which already returns
//! `CratestackError::NotFound` — the exact "dispatch key was never
//! registered" fallback the design calls for (§3, RPC unary row). No
//! new error variant or runtime branch; suppression here is omitting
//! the `quote!` arm, nothing else.

mod arms_read;
mod arms_write;

#[cfg(test)]
mod tests;

use cratestack_core::Model;
use quote::quote;

use crate::shared::{ident, is_primary_key, pluralize, rust_type_tokens, to_snake_case};

/// Everything a single verb's arm builder needs — computed once by
/// [`generate_model_rpc_dispatch_arms`] and shared read-only across all
/// five builders in [`arms_read`]/[`arms_write`].
pub(super) struct ModelRpcContext {
    pub(super) m: String,
    pub(super) list_dispatch: syn::Ident,
    pub(super) create_dispatch: syn::Ident,
    pub(super) get_dispatch: syn::Ident,
    pub(super) update_dispatch: syn::Ident,
    pub(super) delete_dispatch: syn::Ident,
    pub(super) update_input_ident: syn::Ident,
    /// Only populated (and only read by `get`/`update`/`delete`'s arm
    /// builders) once a primary key is confirmed to exist — see the
    /// no-pk early return below.
    pub(super) pk_type: proc_macro2::TokenStream,
}

/// Emit `model.<X>.{list,get,create,update,delete}` dispatch arms.
pub(crate) fn generate_model_rpc_dispatch_arms(model: &Model) -> Vec<proc_macro2::TokenStream> {
    let internal = cratestack_core::model_internal_actions(model);
    let m = model.name.as_str();
    let pk_field = model.fields.iter().find(|field| is_primary_key(field));

    // Models without a primary key can't have get/update/delete ops
    // dispatch (no id to extract). The parser already rejects PK-less
    // models for REST; be defensive here too.
    let Some(pk) = pk_field else {
        return ["list", "get", "create", "update", "delete"]
            .into_iter()
            .filter(|verb| !internal.contains(verb))
            .map(|verb| {
                let op_id = format!("model.{m}.{verb}");
                quote! {
                    #op_id => {
                        rpc_dispatch_error(
                            &state,
                            &headers,
                            ::cratestack::CratestackError::Internal(format!(
                                "model `{}` has no primary key; RPC dispatch impossible",
                                #m,
                            )),
                        )
                    }
                }
            })
            .collect();
    };

    let ctx = ModelRpcContext {
        m: m.to_string(),
        list_dispatch: ident(&format!(
            "handle_list_{}_dispatch",
            pluralize(&to_snake_case(m))
        )),
        create_dispatch: ident(&format!(
            "handle_create_{}_dispatch",
            pluralize(&to_snake_case(m))
        )),
        get_dispatch: ident(&format!("handle_get_{}_dispatch", to_snake_case(m))),
        update_dispatch: ident(&format!("handle_update_{}_dispatch", to_snake_case(m))),
        delete_dispatch: ident(&format!("handle_delete_{}_dispatch", to_snake_case(m))),
        update_input_ident: ident(&format!("Update{m}Input")),
        pk_type: rust_type_tokens(&pk.ty),
    };

    let mut arms = Vec::new();
    if !internal.contains("list") {
        arms.push(arms_read::list_arm(&ctx));
    }
    if !internal.contains("get") {
        arms.push(arms_read::get_arm(&ctx));
    }
    if !internal.contains("create") {
        arms.push(arms_write::create_arm(&ctx));
    }
    if !internal.contains("update") {
        arms.push(arms_write::update_arm(&ctx));
    }
    if !internal.contains("delete") {
        arms.push(arms_write::delete_arm(&ctx));
    }
    arms
}

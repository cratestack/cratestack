//! The static resource table (cratestack#1040): `RESOURCE_OPS`, a
//! `model.<X>.get` and a `model.<X>.list` descriptor per resource, and
//! `RESOURCES`, one `cratestack_mcp::ResourceDescriptor` per resource
//! pointing at them.
//!
//! The descriptors come from the functions that fill RPC's `OPS` for the
//! same verbs (`transport::op_descriptors::reads`), so a resource read is
//! admitted with the flags an RPC read of that model carries. Their
//! `op_id`s name the model, but only for diagnostics: nothing sends them to
//! an agent, whose URIs carry the author's segment instead.

use quote::quote;

use crate::include::mcp_gate::ResourcePlan;
use crate::transport::{model_get_op_descriptor, model_list_op_descriptor};

pub(super) fn resources_table_tokens(
    resources: &[ResourcePlan],
    auth_required: bool,
) -> proc_macro2::TokenStream {
    if resources.is_empty() {
        return proc_macro2::TokenStream::new();
    }
    let count = resources.len();
    let ops = resources.iter().flat_map(|resource| {
        [
            model_get_op_descriptor(&resource.model, auth_required),
            model_list_op_descriptor(&resource.model, auth_required),
        ]
    });
    let op_count = 2 * count;
    let entries = resources.iter().enumerate().map(|(index, resource)| {
        let authority = &resource.authority;
        let segment = &resource.segment;
        let max_page_size = resource.max_page_size;
        let get = 2 * index;
        let list = get + 1;
        quote! {
            ::cratestack::mcp::ResourceDescriptor::new(
                #authority,
                #segment,
                #max_page_size,
                &RESOURCE_OPS[#get],
                &RESOURCE_OPS[#list],
            )
        }
    });

    quote! {
        /// Admission facts per resource: `[get, list]` pairs, index-aligned
        /// with [`RESOURCES`].
        pub static RESOURCE_OPS: [::cratestack::OpDescriptor; #op_count] = [#(#ops),*];

        /// The exposed resources, in declaration order: what
        /// `resources/list` returns, unfiltered by the caller's
        /// authorization (a listing names a kind of record, never a row).
        pub static RESOURCES: [::cratestack::mcp::ResourceDescriptor; #count] = [#(#entries),*];
    }
}

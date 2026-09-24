//! The static tool table: `OPS`, one `OpDescriptor` per tool, and `TOOLS`,
//! one `cratestack_mcp::ToolDescriptor` per tool pointing at it.
//!
//! The descriptors come from [`generate_procedure_op_descriptor`], the same
//! function that fills RPC's `OPS`, which reads `@no_idempotency` and
//! `@no_rate_limit` through the helpers REST's route descriptors share
//! (`transport::idempotency`, `transport::rate_limit`). So MCP admission
//! cannot disagree with what REST and RPC do for the same procedure
//! (cratestack#474's lesson). The `idempotentHint` an agent reads is *not*
//! taken from `idempotent_by_default` any more: a mutation's is always
//! `false` (`cratestack-mcp`'s `listing.rs`, cratestack#1038 decision 3).
//! They are emitted here for every schema, because REST schemas leave
//! `axum::OPS` empty.
//!
//! The JSON Schemas are the phase 2 generator's output, serialized once at
//! expansion time and embedded as `&'static str`.

use cratestack_core::ProcedureKind;
use quote::quote;

use crate::include::mcp_gate::ToolPlan;
use crate::transport::generate_procedure_op_descriptor;

pub(super) fn table_tokens(tools: &[ToolPlan], auth_required: bool) -> proc_macro2::TokenStream {
    let count = tools.len();
    let ops = tools
        .iter()
        .map(|tool| generate_procedure_op_descriptor(&tool.procedure, auth_required));
    let entries = tools.iter().enumerate().map(|(index, tool)| {
        let name = &tool.name;
        let description = match &tool.description {
            Some(text) => quote! { ::core::option::Option::Some(#text) },
            None => quote! { ::core::option::Option::None },
        };
        let input = &tool.input;
        let output = match &tool.output {
            Some(schema) => quote! { ::core::option::Option::Some(#schema) },
            None => quote! { ::core::option::Option::None },
        };
        let read_only = matches!(tool.procedure.kind, ProcedureKind::Query);
        quote! {
            ::cratestack::mcp::ToolDescriptor::new(
                #name,
                #description,
                #input,
                #output,
                #read_only,
                &OPS[#index],
            )
        }
    });

    quote! {
        /// Admission facts per tool, index-aligned with [`TOOLS`].
        pub static OPS: [::cratestack::OpDescriptor; #count] = [#(#ops),*];

        /// The exposed tools, in declaration order: what `tools/list`
        /// returns, unfiltered by the caller's authorization.
        pub static TOOLS: [::cratestack::mcp::ToolDescriptor; #count] = [#(#entries),*];
    }
}

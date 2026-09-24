//! The code a `tools/call` runs: `Call` (one variant per tool, holding the
//! procedure's typed `Args`), the `McpTools` value the application builds,
//! and its `cratestack_mcp::McpTools` impl — `decode` matches the tool name
//! to the right `Args`, `execute` matches the variant to the procedure.
//!
//! **Policy.** Each `execute` arm is the REST handler's call, restated
//! without HTTP (`crate::axum::procedure`): `invoke_with_db(&db, &args,
//! ctx, |authorized| registry.<method>(&db, &ctx, args, authorized))`. The
//! witness is the only way to call the registry method, and only
//! `invoke_with_db` can make one, so there is no arm shape that reaches an
//! implementation without its `@allow` (cratestack#512).
//!
//! **Computed output fields** (ADR 0002 Q7) go through
//! [`compose_tail_tokens`], the exact tokens the REST/RPC dispatch tail
//! emits, calling the same `compose_<owner>_value` helpers in the generated
//! `axum` module. The tail reads `state.db`, `state.resolvers` and `ctx`,
//! so `execute` binds `state` to itself to give it those names.

use std::collections::BTreeSet;

use cratestack_core::pascal_case::to_pascal_case;
use quote::quote;

use crate::axum::compose_tail_tokens;
use crate::computed::{ProcedureOutputComposition, compose_fn_ident, procedure_output_composition};
use crate::include::mcp_gate::ToolPlan;
use crate::shared::{ident, is_stream_procedure, to_snake_case};

pub(super) fn dispatch_tokens(
    tools: &[ToolPlan],
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let variants: Vec<_> = tools
        .iter()
        .map(|tool| ident(&to_pascal_case(&tool.procedure.name)))
        .collect();
    let modules: Vec<_> = tools
        .iter()
        .map(|tool| ident(&to_snake_case(&tool.procedure.name)))
        .collect();
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    let arms = tools
        .iter()
        .zip(&variants)
        .zip(&modules)
        .map(|((tool, variant), module)| execute_arm(tool, variant, module, bearing));
    let compose_imports = compose_imports(tools, bearing);

    quote! {
        use ::cratestack::CratestackError;
        #compose_imports

        /// One decoded tool call.
        pub enum Call {
            #(#variants(super::procedures::#modules::Args),)*
        }

        /// The schema's tool table, for `::cratestack::mcp::StdioServer`.
        #[derive(Clone)]
        pub struct McpTools<R, CR> {
            db: super::Cratestack,
            registry: R,
            resolvers: CR,
        }

        /// Build the tool table from what the REST router is built from.
        pub fn tools<R, CR>(db: super::Cratestack, registry: R, resolvers: CR) -> McpTools<R, CR>
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
        {
            McpTools { db, registry, resolvers }
        }

        impl<R, CR> ::cratestack::mcp::McpTools for McpTools<R, CR>
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
        {
            type Call = Call;

            fn tools(&self) -> &'static [::cratestack::mcp::ToolDescriptor] {
                &TOOLS
            }

            fn decode(
                &self,
                tool: &str,
                arguments: ::cratestack::serde_json::Value,
            ) -> ::core::result::Result<Call, ::cratestack::mcp::ArgumentsError> {
                match tool {
                    #(#names => ::cratestack::mcp::decode_arguments(arguments).map(Call::#variants),)*
                    other => ::core::result::Result::Err(::cratestack::mcp::ArgumentsError::new(
                        ::std::format!("unknown tool `{other}`"),
                    )),
                }
            }

            async fn execute(
                &self,
                call: Call,
                ctx: &::cratestack::CratestackContext,
            ) -> ::core::result::Result<::cratestack::serde_json::Value, CratestackError> {
                let state = self;
                match call {
                    #(#arms)*
                }
            }
        }
    }
}

fn execute_arm(
    tool: &ToolPlan,
    variant: &syn::Ident,
    module: &syn::Ident,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    if is_stream_procedure(&tool.procedure) {
        // Unreachable: the parser refuses `@mcp(tool)` on `@stream` (ADR
        // 0002 Q8). Kept loud rather than silently buffering a stream if
        // that rule were ever lost.
        let message = format!(
            "`@mcp(tool)` on `@stream` procedure `{}` (ADR 0002 Q8)",
            tool.procedure.name
        );
        return quote! { Call::#variant(_) => ::core::compile_error!(#message), };
    }
    let compose_tail = compose_tail_tokens(procedure_output_composition(
        &tool.procedure.return_type,
        bearing,
    ));
    quote! {
        Call::#variant(args) => {
            let registry = state.registry.clone();
            let db = state.db.clone();
            let call_ctx = ctx.clone();
            let call_args = args.clone();
            let result = super::procedures::#module::invoke_with_db(
                &state.db,
                &args,
                ctx,
                |authorized| async move {
                    registry.#module(&db, &call_ctx, call_args, authorized).await
                },
            )
            .await;
            #compose_tail
            ::cratestack::mcp::encode_output(&result?)
        }
    }
}

/// `use super::axum::{compose_<owner>_value, ...};` for the owners a tool's
/// output composes at its top level (nested owners are composed inside
/// those helpers). Nothing when no tool's output is computed-bearing.
fn compose_imports(tools: &[ToolPlan], bearing: &BTreeSet<String>) -> proc_macro2::TokenStream {
    let owners: BTreeSet<String> = tools
        .iter()
        .filter_map(|tool| procedure_output_composition(&tool.procedure.return_type, bearing))
        .map(|composition| match composition {
            ProcedureOutputComposition::Unary { owner, .. }
            | ProcedureOutputComposition::List { owner }
            | ProcedureOutputComposition::Page { owner } => owner,
        })
        .collect();
    if owners.is_empty() {
        return proc_macro2::TokenStream::new();
    }
    let helpers = owners.iter().map(|owner| compose_fn_ident(owner));
    quote! { use super::axum::{#(#helpers),*}; }
}

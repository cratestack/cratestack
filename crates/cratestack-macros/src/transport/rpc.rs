//! RPC dispatch arms emitted into the body of the generated
//! `rpc_dispatch` fn. Each model verb constructs a `ModelRouterState`
//! from the unified `RpcRouterState`, decodes the RPC body into the
//! right input shape, and delegates to the verb's `_dispatch` fn —
//! passing a `CanonicalRequest` describing the ACTUAL rpc request
//! (`POST /rpc/model.<M>.<verb>`, no query, the raw frame bytes) as the
//! canonical signed identity. On `transport rpc` that concrete rpc URL +
//! frame body is the single identity for url, dispatch, signing, and
//! tracing — it matches the rpc client byte-for-byte and the REST
//! `/<plural>` shape never appears.
//!
//! The per-verb arm builders live in [`model_dispatch`] (split out to
//! stay under this crate's 200-LoC file convention — five verb arms,
//! each carrying its own decode/dispatch shape, don't fit in one file
//! alongside the orchestration that filters them by
//! `cratestack_core::model_internal_actions`, cratestack#743).

mod model_dispatch;

pub(crate) use model_dispatch::generate_model_rpc_dispatch_arms;

use cratestack_core::Procedure;
use quote::quote;

use crate::shared::{ident, to_snake_case};

pub(crate) fn generate_procedure_rpc_dispatch_arm(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    let op_id = format!("procedure.{}", procedure.name);
    let canonical_path = format!("/rpc/{op_id}");
    let dispatch_ident = ident(&format!(
        "handle_{}_dispatch",
        to_snake_case(&procedure.name)
    ));
    quote! {
        #op_id => {
            // The canonical signed request IS the actual rpc request:
            // `POST /rpc/procedure.<name>` with the raw frame bytes. This
            // matches the rpc client byte-for-byte; `/$procs/<name>` never
            // appears on `transport rpc`.
            let canonical_body = body.clone();
            #dispatch_ident(
                ProcedureRouterState {
                    db: state.db.clone(),
                    registry: state.registry.clone(),
                    resolvers: state.resolvers.clone(),
                    codec: state.codec.clone(),
                    auth_provider: state.auth_provider.clone(),
                },
                CanonicalRequest {
                    method: "POST",
                    path: #canonical_path,
                    query: None,
                    body: canonical_body.as_ref(),
                },
                headers,
                client_ip_ctx,
                body,
            ).await
        }
    }
}

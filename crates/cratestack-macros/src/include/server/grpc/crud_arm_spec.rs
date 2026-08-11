//! [`build_unary_arm`] — the one shape shared by all five CRUD match arms
//! `crud_arms.rs` builds (marker struct, `impl UnaryService`,
//! `Future`/`Response` associated types, `Box::pin(... grpc.unary(svc,
//! req).await)` tail). Split out of `crud_arms.rs` to keep that file
//! under this repo's 200-LoC convention; see `service.rs`'s module doc
//! for what this shape mirrors and why (cratestack#426 — this used to be
//! independently reimplemented five times).

use quote::quote;

use super::arm_support::request_prelude;

pub(super) fn method_path(package: &str, model: &str, verb_pascal: &str) -> String {
    format!("/{package}.Api/Model{model}{verb_pascal}")
}

/// `super::axum::ModelRouterState<C, Auth>` built ad hoc from the
/// `ProcedureRouterState<R, C, Auth>` `ApiServer` actually carries —
/// mirrors `crate::transport::rpc::generate_model_rpc_dispatch_arms`,
/// which does exactly this to call a model `_dispatch` fn from the
/// unified `RpcRouterState` the RPC binding threads through instead.
fn model_state_from_procedure_state() -> proc_macro2::TokenStream {
    quote! {
        super::axum::ModelRouterState {
            db: state.db.clone(),
            codec: state.codec.clone(),
            auth_provider: state.auth_provider.clone(),
        }
    }
}

/// What each `build_*_arm` function in `crud_arms.rs` supplies: the pb
/// wire types (`request_ty`/`response_ty`), the marker struct's name, and
/// `body` — the decode -> dispatch -> bridge sequence that's the only
/// part genuinely unique per CRUD verb. Everything else about the arm is
/// identical across all five and lives once in [`build_unary_arm`].
pub(super) struct ArmSpec {
    pub(super) path: String,
    pub(super) request_ty: proc_macro2::Ident,
    pub(super) response_ty: proc_macro2::Ident,
    pub(super) svc_ident: proc_macro2::Ident,
    pub(super) body: proc_macro2::TokenStream,
}

pub(super) fn build_unary_arm(spec: ArmSpec) -> proc_macro2::TokenStream {
    let ArmSpec {
        path,
        request_ty,
        response_ty,
        svc_ident,
        body,
    } = spec;
    let prelude = request_prelude(&path);
    let model_state = model_state_from_procedure_state();
    quote! {
        #path => {
            struct #svc_ident<C, Auth>(super::axum::ModelRouterState<C, Auth>);
            impl<C, Auth> ::cratestack::grpc::tonic::server::UnaryService<pb::#request_ty> for #svc_ident<C, Auth>
            where
                C: ::cratestack::HttpTransport + Send + Sync + 'static,
                Auth: ::cratestack::AuthProvider + Send + Sync + 'static,
            {
                type Response = pb::#response_ty;
                type Future = ::cratestack::grpc::tonic::codegen::BoxFuture<
                    ::cratestack::grpc::tonic::Response<Self::Response>,
                    ::cratestack::grpc::tonic::Status,
                >;
                fn call(&mut self, request: ::cratestack::grpc::tonic::Request<pb::#request_ty>) -> Self::Future {
                    let state = self.0.clone();
                    Box::pin(async move {
                        #prelude
                        #body
                    })
                }
            }
            let svc = #svc_ident(#model_state);
            let codec = ::cratestack::grpc::tonic::codec::ProstCodec::default();
            let mut grpc = ::cratestack::grpc::tonic::server::Grpc::new(codec);
            Box::pin(async move { Ok(grpc.unary(svc, req).await) })
        }
    }
}

//! The top-level `router()` entrypoint (cratestack#328). Under
//! `db = Postgres` it merges `model_router` + `procedure_router` as
//! before. Under `db = None` there is no `model_router` at all (see
//! `super::model_router`'s module doc), so `router()` is simply an alias
//! for `procedure_router` — schema-author bootstrap code calls the same
//! `router(db, registry, codec, auth_provider, body_limit_bytes)`
//! entrypoint regardless of `db` mode, it just merges nothing under
//! `db = None`.
//!
//! `body_limit_bytes` (cratestack#413) is applied here — once, as the
//! outermost `DefaultBodyLimit` layer around the fully-merged router —
//! rather than inside `model_router`/`procedure_router` individually, and
//! rather than left for a consumer to re-layer after the fact. Both of
//! those alternatives were tried and rejected: `axum::extract::
//! DefaultBodyLimit` is extension-based and unconditionally overwrites
//! its extension on every layer invocation, so whichever `DefaultBodyLimit`
//! sits closest to the handler always wins — a consumer re-layering on
//! top of a value already baked in deeper cannot override it, in either
//! direction. See `crates/cratestack-core/src/limits.rs`'s module doc and
//! `docs/design/request-response-size-bounds.md` Decision 2 for the
//! empirical reproduction. Passing the limit as a real parameter here
//! means exactly one `DefaultBodyLimit` layer is ever constructed, with
//! the caller's chosen value baked in once — a consumer who wants a
//! different ceiling passes a different `body_limit_bytes`, which
//! actually takes effect. `rpc_module.rs`'s `rpc_router()` applies the
//! same parameter the same way for `transport rpc` schemas, since the two
//! entrypoints don't share a merge point (cratestack#413's root cause for
//! the batch frame cap applies to this surface too — see that file).

use quote::quote;

use super::super::super::parse::ServerDb;

pub(super) fn build(db: ServerDb) -> proc_macro2::TokenStream {
    match db {
        ServerDb::Postgres => quote! {
            pub fn router<R, CR, C, Auth>(
                db: super::Cratestack,
                registry: R,
                resolvers: CR,
                codec: C,
                auth_provider: Auth,
                body_limit_bytes: usize,
            ) -> axum::Router
            where
                R: super::procedures::ProcedureRegistry,
                CR: super::computed::ComputedFieldResolver,
                C: HttpTransport,
                Auth: AuthProvider,
            {
                model_router(db.clone(), resolvers.clone(), codec.clone(), auth_provider.clone())
                    .merge(procedure_router(db, registry, resolvers, codec, auth_provider))
                    .layer(::cratestack::axum::extract::DefaultBodyLimit::max(body_limit_bytes))
            }
        },
        ServerDb::None => quote! {
            pub fn router<R, CR, C, Auth>(
                db: super::Cratestack,
                registry: R,
                resolvers: CR,
                codec: C,
                auth_provider: Auth,
                body_limit_bytes: usize,
            ) -> axum::Router
            where
                R: super::procedures::ProcedureRegistry,
                CR: super::computed::ComputedFieldResolver,
                C: HttpTransport,
                Auth: AuthProvider,
            {
                procedure_router(db, registry, resolvers, codec, auth_provider)
                    .layer(::cratestack::axum::extract::DefaultBodyLimit::max(body_limit_bytes))
            }
        },
    }
}

//! The top-level `router()` entrypoint (cratestack#328). Under
//! `db = Postgres` it merges `model_router` + `procedure_router` as
//! before. Under `db = None` there is no `model_router` at all (see
//! `super::model_router`'s module doc), so `router()` is simply an alias
//! for `procedure_router` — schema-author bootstrap code calls the same
//! `router(db, registry, codec, auth_provider)` entrypoint regardless of
//! `db` mode, it just merges nothing under `db = None`.

use quote::quote;

use super::super::super::parse::ServerDb;

pub(super) fn build(db: ServerDb) -> proc_macro2::TokenStream {
    match db {
        ServerDb::Postgres => quote! {
            pub fn router<R, C, Auth>(
                db: super::Cratestack,
                registry: R,
                codec: C,
                auth_provider: Auth,
            ) -> axum::Router
            where
                R: super::procedures::ProcedureRegistry,
                C: HttpTransport,
                Auth: AuthProvider,
            {
                model_router(db.clone(), codec.clone(), auth_provider.clone())
                    .merge(procedure_router(db, registry, codec, auth_provider))
            }
        },
        ServerDb::None => quote! {
            pub fn router<R, C, Auth>(
                db: super::Cratestack,
                registry: R,
                codec: C,
                auth_provider: Auth,
            ) -> axum::Router
            where
                R: super::procedures::ProcedureRegistry,
                C: HttpTransport,
                Auth: AuthProvider,
            {
                procedure_router(db, registry, codec, auth_provider)
            }
        },
    }
}

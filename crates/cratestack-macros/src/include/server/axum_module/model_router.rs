//! `ModelRouterState` + `model_router` (cratestack#328). Emitted only for
//! `db = Postgres`: `datasource { provider = "none" }` schemas can never
//! declare a `model` (cratestack#327's guard), so under `db = None` this
//! whole surface would be dead — a struct nothing constructs and a fn
//! nothing calls. Returning an empty [`proc_macro2::TokenStream`] for
//! `ServerDb::None` makes both compile out entirely rather than exist as
//! unused generic code.

use quote::quote;

use super::super::super::parse::ServerDb;

type Ts = proc_macro2::TokenStream;

pub(super) fn build_state(db: ServerDb) -> Ts {
    match db {
        ServerDb::Postgres => quote! {
            #[derive(Clone)]
            pub struct ModelRouterState<CR, C, Auth> {
                pub db: super::Cratestack,
                pub resolvers: CR,
                pub codec: C,
                pub auth_provider: Auth,
            }
        },
        ServerDb::None => Ts::new(),
    }
}

pub(super) fn build_fn(db: ServerDb, model_axum_routes: &[Ts]) -> Ts {
    match db {
        ServerDb::Postgres => quote! {
            pub fn model_router<CR, C, Auth>(
                db: super::Cratestack,
                resolvers: CR,
                codec: C,
                auth_provider: Auth,
            ) -> axum::Router
            where
                CR: super::computed::ComputedFieldResolver,
                C: HttpTransport,
                Auth: AuthProvider,
            {
                let state = ModelRouterState {
                    db,
                    resolvers,
                    codec,
                    auth_provider,
                };

                axum::Router::new()
                    #(#model_axum_routes)*
                    .layer(::cratestack::axum::middleware::from_fn_with_state(
                        super::SCHEMA_SHA256,
                        ::cratestack::schema_fingerprint::warn_on_schema_mismatch,
                    ))
                    .with_state(state)
            }
        },
        ServerDb::None => Ts::new(),
    }
}

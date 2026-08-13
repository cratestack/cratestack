//! `db = None` runtime block (cratestack#328): a `Cratestack` type that
//! carries no `PgPool`, no `SqlxRuntime` — genuinely nothing
//! database-shaped, not an `Option<PgPool>` that happens to always be
//! `None`. There is no `.pool()` method and no way to reach a `PgPool`
//! from this type at all.
//!
//! `Cratestack` still exists (as a zero-field marker) because procedure
//! dispatch (`ProcedureRegistry` trait methods, generated in
//! `crate::procedure`) and policy evaluation (`authorize_with_db`,
//! generated in `crate::procedure::instrument`) both take `&Cratestack`
//! unconditionally — that code is shared between `db = Postgres` and
//! `db = None` on purpose (see this story's PR description for why a
//! forked dispatch path wasn't worth it: procedure policies never touch
//! `PgPool`, verified by grep across `crate::policy`). Keeping one shared
//! `&Cratestack` parameter shape means `crate::axum::procedure` and
//! `crate::procedure::instrument` don't need a `db`-conditional branch of
//! their own; only the *type behind* `Cratestack` changes.
//!
//! `events()`/`views()` are omitted entirely rather than kept as
//! always-empty API surface: `datasource { provider = "none" }` schemas
//! can never declare a `model` (cratestack#327), so there is provably
//! nothing to subscribe to or view.

use quote::quote;

pub(super) fn build_runtime_block() -> proc_macro2::TokenStream {
    quote! {
        /// No-database runtime handle (`db = None`). See this module's
        /// doc comment for why this is a zero-field marker rather than an
        /// `Option`-wrapped pool.
        #[derive(Clone, Copy)]
        pub struct Cratestack;

        #[derive(Clone)]
        pub struct BoundCratestack {
            ctx: ::cratestack::CoolContext,
        }

        pub struct CratestackBuilder;

        impl Cratestack {
            pub fn builder() -> CratestackBuilder {
                CratestackBuilder
            }

            pub fn bind_context(&self, ctx: ::cratestack::CoolContext) -> BoundCratestack {
                BoundCratestack { ctx }
            }

            pub fn bind_auth<P: ::cratestack::serde::Serialize>(
                &self,
                principal: Option<P>,
            ) -> Result<BoundCratestack, ::cratestack::CoolError> {
                let ctx = ::cratestack::CoolContext::from_principal(principal)?;
                Ok(self.bind_context(ctx))
            }
        }

        impl BoundCratestack {
            pub fn context(&self) -> &::cratestack::CoolContext {
                &self.ctx
            }
        }

        impl CratestackBuilder {
            pub fn build(self) -> Cratestack {
                Cratestack
            }
        }

        pub fn schema_summary() -> ::cratestack::SchemaSummary {
            ::cratestack::SchemaSummary {
                mixins: MIXINS,
                models: MODELS,
                types: TYPES,
                enums: ENUMS,
                procedures: PROCEDURES,
                views: VIEWS,
            }
        }
    }
}

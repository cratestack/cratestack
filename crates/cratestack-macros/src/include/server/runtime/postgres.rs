//! `db = Postgres` runtime block: `Cratestack` (the sqlx-backed delegate
//! hub), `BoundCratestack` (context-bound view), `CratestackBuilder`, plus
//! `schema_summary()`. Byte-identical to the pre-cratestack#328 output —
//! this file is the exact original `build_runtime_block` body, just moved
//! under `runtime/` alongside its `db = None` sibling
//! ([`super::none::build_runtime_block`]).

use quote::quote;

pub(super) fn build_runtime_block(
    model_accessors: &[proc_macro2::TokenStream],
    bound_model_accessors: &[proc_macro2::TokenStream],
    view_accessors: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    let views_module = quote! {
        pub mod views {
            //! View sub-accessor (ADR-0003). `runtime.views()` returns
            //! a `Views<'_>` whose methods hand out `ViewDelegate`s for
            //! each `view` block declared in the schema.
            pub struct Views<'a> {
                pub(super) runtime: &'a ::cratestack::__private::SqlxRuntime,
            }

            impl<'a> Views<'a> {
                pub(super) fn new(runtime: &'a ::cratestack::__private::SqlxRuntime) -> Self {
                    Self { runtime }
                }

                #(#view_accessors)*
            }
        }
    };

    quote! {
        #[derive(Clone)]
        pub struct Cratestack {
            runtime: ::cratestack::__private::SqlxRuntime,
        }

        #[derive(Clone)]
        pub struct BoundCratestack<'a> {
            inner: &'a Cratestack,
            ctx: ::cratestack::CoolContext,
        }

        pub struct CratestackBuilder {
            runtime: ::cratestack::__private::SqlxRuntime,
        }

        impl Cratestack {
            pub fn builder(pool: ::cratestack::sqlx::PgPool) -> CratestackBuilder {
                CratestackBuilder {
                    runtime: ::cratestack::__private::SqlxRuntime::new(pool),
                }
            }

            pub fn bind_context(&self, ctx: ::cratestack::CoolContext) -> BoundCratestack<'_> {
                BoundCratestack { inner: self, ctx }
            }

            pub fn pool(&self) -> &::cratestack::sqlx::PgPool {
                self.runtime.pool()
            }

            /// Compose several writes in one Postgres transaction without
            /// naming a `sqlx` type (cratestack#513): `body` receives an
            /// opaque `::cratestack::Tx` it can pass straight to any write
            /// builder's `run_in_tx` (e.g.
            /// `self.some_model().create(..).run_in_tx(tx, &ctx)`); commits
            /// on `Ok`, rolls back on `Err`. See
            /// `::cratestack::__private::SqlxRuntime::transaction`'s doc
            /// comment for the full rollback-timing and no-retry rationale.
            pub async fn transaction<F, T>(
                &self,
                body: F,
            ) -> Result<T, ::cratestack::CoolError>
            where
                F: AsyncFnOnce(&mut ::cratestack::Tx) -> Result<T, ::cratestack::CoolError>,
            {
                self.runtime.transaction(body).await
            }

            pub fn bind_auth<P: ::cratestack::serde::Serialize>(
                &self,
                principal: Option<P>,
            ) -> Result<BoundCratestack<'_>, ::cratestack::CoolError> {
                let ctx = ::cratestack::CoolContext::from_principal(principal)?;
                Ok(self.bind_context(ctx))
            }

            #(#model_accessors)*

            pub fn events(&self) -> events::Subscriptions<'_> {
                events::Subscriptions::new(&self.runtime)
            }

            pub fn views(&self) -> views::Views<'_> {
                views::Views::new(&self.runtime)
            }
        }

        impl<'a> BoundCratestack<'a> {
            pub fn context(&self) -> &::cratestack::CoolContext {
                &self.ctx
            }

            #(#bound_model_accessors)*
        }

        impl CratestackBuilder {
            /// Install a custom [`::cratestack::AuditSink`] (cratestack#473) —
            /// every `@@audit` mutation run through the built `Cratestack`
            /// fans out to it, in addition to the always-on
            /// `cratestack_audit` table row. See
            /// `::cratestack::__private::SqlxRuntime::with_audit_sink`'s doc
            /// comment for the in-transaction-vs-post-commit dispatch
            /// contract.
            pub fn with_audit_sink(
                mut self,
                sink: ::std::sync::Arc<dyn ::cratestack::AuditSink>,
            ) -> Self {
                self.runtime = self.runtime.with_audit_sink(sink);
                self
            }

            pub fn build(self) -> Cratestack {
                Cratestack {
                    runtime: self.runtime,
                }
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

        #views_module
    }
}

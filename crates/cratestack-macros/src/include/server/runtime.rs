//! Runtime types emitted inside `pub mod cratestack_schema { ... }`:
//! `Cratestack` (the sqlx-backed delegate hub), `BoundCratestack`
//! (context-bound view), `CratestackBuilder`, plus `schema_summary()`.

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

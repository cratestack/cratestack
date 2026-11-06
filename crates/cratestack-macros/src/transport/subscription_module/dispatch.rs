//! `SubscriptionDispatch` itself: the struct, how a `Cratestack` builds one,
//! the `SubscriptionDispatcher` impl, and `open_query`'s op-id match.
//!
//! With subscription procedures the dispatcher holds the schema's
//! `ProcedureRegistry` (`SubscriptionDispatch<R>`, from
//! `Cratestack::subscriptions(registry)`); without, `R` is `()` and
//! `Cratestack::subscriptions()` takes nothing, as before slice S4.

use quote::quote;

/// `(impl generics, the dispatcher's type)` for an `impl` block.
pub(super) fn generics(registry: bool) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    if registry {
        (
            quote! { <R: super::procedures::ProcedureRegistry> },
            quote! { SubscriptionDispatch<R> },
        )
    } else {
        (quote! {}, quote! { SubscriptionDispatch })
    }
}

pub(super) fn tokens(
    registry: bool,
    postgres: bool,
    arms: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let (generics, ty) = generics(registry);
    let constructor = if registry {
        quote! {
            impl #generics #ty {
                pub fn new(db: super::Cratestack, registry: R) -> Self {
                    Self { db, registry }
                }
            }

            impl super::Cratestack {
                /// This schema's subscription dispatcher, for a binding
                /// other than the generated SSE endpoints. `registry`
                /// serves the `subscription procedure`s.
                pub fn subscriptions<R: super::procedures::ProcedureRegistry>(
                    &self,
                    registry: R,
                ) -> SubscriptionDispatch<R> {
                    SubscriptionDispatch::new(self.clone(), registry)
                }
            }
        }
    } else {
        quote! {
            impl SubscriptionDispatch {
                pub fn new(db: super::Cratestack) -> Self {
                    Self { db, registry: () }
                }
            }

            impl super::Cratestack {
                /// This schema's subscription dispatcher, for a binding
                /// other than the generated SSE endpoints.
                pub fn subscriptions(&self) -> SubscriptionDispatch {
                    SubscriptionDispatch::new(self.clone())
                }
            }
        }
    };
    let input_query = if registry {
        quote! {
            if op_id.starts_with("procedure.") {
                ::cratestack::__private::procedure_input_query(input)?
            } else {
                ::cratestack::__private::subscribe_input_query(input)?
            }
        }
    } else {
        quote! { ::cratestack::__private::subscribe_input_query(input)? }
    };
    let backplane = postgres.then(backplane_builder);
    quote! {
        /// Opens this schema's subscriptions by op id. Get one with
        /// `Cratestack::subscriptions`.
        #[derive(Clone)]
        pub struct SubscriptionDispatch<R = ()> {
            db: super::Cratestack,
            #[allow(dead_code)]
            registry: R,
        }

        #constructor

        #backplane

        impl #generics ::cratestack::SubscriptionDispatcher for #ty {
            fn open(
                &self,
                request: ::cratestack::SubscriptionOpen,
            ) -> impl ::core::future::Future<
                Output = Result<
                    ::cratestack::SubscriptionStream<::cratestack::serde_json::Value>,
                    ::cratestack::CratestackError,
                >,
            > + Send {
                let this = self.clone();
                async move {
                    let ::cratestack::SubscriptionOpen { op_id, input, ctx } = request;
                    let query = #input_query;
                    this.open_query(&op_id, query, ctx).await
                }
            }
        }

        impl #generics #ty {
            /// Opens `op_id` with its input as a query string: a model
            /// feed's `list` filters (`status=open&operations=created`), a
            /// subscription procedure's JSON-valued arguments
            /// (`args={"roomId":"r1"}`).
            pub async fn open_query(
                &self,
                op_id: &str,
                query: Option<String>,
                ctx: ::cratestack::CratestackContext,
            ) -> Result<
                ::cratestack::SubscriptionStream<::cratestack::serde_json::Value>,
                ::cratestack::CratestackError,
            > {
                match op_id {
                    #arms
                    other => Err(::cratestack::CratestackError::NotFound(format!(
                        "unknown subscription op `{other}`",
                    ))),
                }
            }
        }
    }
}

fn backplane_builder() -> proc_macro2::TokenStream {
    quote! {
        impl super::CratestackBuilder {
            /// Replace the subscription backplane, which carries model
            /// events and topic messages to subscribers on every replica
            /// (default: `PgNotifyBackplane`, Postgres `LISTEN`/`NOTIFY` on
            /// this pool). Pass an `InProcessBackplane` for a single replica
            /// whose connections cannot hold `LISTEN` (PgBouncer in
            /// transaction mode).
            pub fn with_backplane(
                mut self,
                backplane: ::std::sync::Arc<dyn ::cratestack::SubscriptionBackplane>,
            ) -> Self {
                self.runtime = self.runtime.with_backplane(backplane);
                self
            }
        }
    }
}

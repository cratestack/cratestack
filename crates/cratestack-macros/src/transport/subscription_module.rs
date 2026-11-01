//! The schema's `subscriptions` module: one generated
//! `cratestack_core::SubscriptionDispatcher` behind every subscription
//! binding (`docs/design/subscriptions-websocket.md` §6.1, slice S5a).
//!
//! `SubscriptionDispatch::open` matches the op id and opens the
//! subscription: policy, setup, event-bus registration, the per-event read
//! policy ([`model_feed`]). A binding only moves frames: the SSE handler
//! (`super::subscribe_dispatch`) authenticates, opens through this
//! dispatcher and encodes; the WebTransport server
//! (`cratestack-webtransport`) is handed the same dispatcher. So the policy
//! code exists once, whatever the transport.
//!
//! Emitted for `db = Postgres` server schemas with at least one
//! `@@subscribe` model (only model feeds exist until slice S4 adds
//! subscription procedures). `extension webtransport { }` adds
//! `SubscriptionDispatch::webtransport`, which binds the WebTransport
//! server to this dispatcher; declaring it with no subscription op to serve
//! is a compile error rather than an extension that silently does nothing.

mod model_feed;

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

pub(crate) fn generate_subscription_module(
    models: &[Model],
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
    webtransport: bool,
) -> Result<proc_macro2::TokenStream, String> {
    let mut feeds = Vec::new();
    for model in models {
        if let Some(feed) = model_feed::generate(model, model_names, enum_names)? {
            feeds.push(feed);
        }
    }
    if feeds.is_empty() {
        return if webtransport {
            Err(
                "schema declares `extension webtransport { }`, but has no subscription op to \
                 serve over it: add `@@subscribe` to a model (under `transport rpc`), or drop \
                 the extension block"
                    .to_owned(),
            )
        } else {
            Ok(proc_macro2::TokenStream::new())
        };
    }
    let arms = feeds.iter().map(|feed| {
        let (op_id, open) = (&feed.op_id, &feed.open_ident);
        quote! { #op_id => #open(&db, input, ctx).await, }
    });
    let functions = feeds.iter().map(|feed| &feed.function);
    let webtransport_impl = webtransport.then(webtransport_impl);

    Ok(quote! {
        /// The schema's subscription ops behind one transport-neutral
        /// [`::cratestack::SubscriptionDispatcher`]: the SSE endpoint opens
        /// through it, and so does any other binding it is handed to.
        pub mod subscriptions {
            /// Opens this schema's subscriptions by op id. Get one with
            /// `Cratestack::subscriptions()`.
            #[derive(Clone)]
            pub struct SubscriptionDispatch {
                db: super::Cratestack,
            }

            impl SubscriptionDispatch {
                pub fn new(db: super::Cratestack) -> Self {
                    Self { db }
                }
            }

            impl super::Cratestack {
                /// This schema's subscription dispatcher, for a binding
                /// other than the generated SSE endpoint.
                pub fn subscriptions(&self) -> SubscriptionDispatch {
                    SubscriptionDispatch::new(self.clone())
                }
            }

            impl super::CratestackBuilder {
                /// Replace the subscription backplane, which carries model
                /// events to subscribers on every replica (default:
                /// `PgNotifyBackplane`, Postgres `LISTEN`/`NOTIFY` on this
                /// pool). Pass an `InProcessBackplane` for a single replica
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

            impl ::cratestack::SubscriptionDispatcher for SubscriptionDispatch {
                fn open(
                    &self,
                    request: ::cratestack::SubscriptionOpen,
                ) -> impl ::core::future::Future<
                    Output = Result<
                        ::cratestack::SubscriptionStream<::cratestack::serde_json::Value>,
                        ::cratestack::CratestackError,
                    >,
                > + Send {
                    let db = self.db.clone();
                    async move {
                        let ::cratestack::SubscriptionOpen { op_id, input, ctx } = request;
                        match op_id.as_str() {
                            #(#arms)*
                            other => Err(::cratestack::CratestackError::NotFound(format!(
                                "unknown subscription op `{other}`",
                            ))),
                        }
                    }
                }
            }

            /// A model feed takes no input until slice S3 adds its filters;
            /// an empty object is the same as none.
            fn no_input(
                op_id: &str,
                input: &::cratestack::serde_json::Value,
            ) -> Result<(), ::cratestack::CratestackError> {
                match input {
                    ::cratestack::serde_json::Value::Null => Ok(()),
                    ::cratestack::serde_json::Value::Object(map) if map.is_empty() => Ok(()),
                    _ => Err(::cratestack::CratestackError::BadRequest(format!(
                        "`{op_id}` takes no input",
                    ))),
                }
            }

            #(#functions)*

            #webtransport_impl
        }
    })
}

fn webtransport_impl() -> proc_macro2::TokenStream {
    quote! {
        impl SubscriptionDispatch {
            /// Binds a WebTransport server (`extension webtransport { }`)
            /// that serves this schema's subscriptions, authenticating
            /// sessions with `auth`, normally the router's own provider.
            /// Run it with `serve` or `serve_with_shutdown`.
            pub fn webtransport<A: ::cratestack::AuthProvider>(
                self,
                config: ::cratestack::webtransport::WebTransportConfig,
                auth: A,
            ) -> Result<
                ::cratestack::webtransport::WebTransportServer<A, Self>,
                ::cratestack::CratestackError,
            > {
                ::cratestack::webtransport::WebTransportServer::bind(config, auth, self)
            }
        }
    }
}

#[cfg(test)]
mod tests;

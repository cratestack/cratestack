//! The schema's `subscriptions` module: one generated
//! `cratestack_core::SubscriptionDispatcher` behind every subscription
//! binding (`docs/design/subscriptions-websocket.md` §6.1, slice S5a).
//!
//! `SubscriptionDispatch::open` matches the op id and opens the
//! subscription: policy, setup, event-bus registration, the per-event read
//! policy ([`model_feed`]). A binding only moves frames: the SSE handler
//! (`include/server/rpc_module/subscribe.rs`) authenticates, opens through this
//! dispatcher and encodes; the WebTransport server
//! (`cratestack-webtransport`) is handed the same dispatcher. So the policy
//! code exists once, whatever the transport.
//!
//! Emitted for a server schema with at least one subscription op: an
//! `@@subscribe` model (`db = Postgres`) or a `subscription procedure`
//! (slice S4, either `db`). With subscription procedures the dispatcher
//! holds the `ProcedureRegistry` their handlers live on, so it is
//! `SubscriptionDispatch<R>`, built by `Cratestack::subscriptions(registry)`;
//! without, it is built by `Cratestack::subscriptions()`.
//! `extension webtransport { }` adds
//! `SubscriptionDispatch::webtransport`, which binds the WebTransport
//! server to this dispatcher; declaring it with no subscription op to serve
//! is a compile error rather than an extension that silently does nothing.

mod dispatch;
mod feed_query;
mod model_feed;
mod procedure_feed;

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure};
use quote::quote;

/// What the schema declares that the module serves.
pub(crate) struct SubscriptionSchema<'a> {
    pub(crate) models: &'a [Model],
    pub(crate) model_names: &'a BTreeSet<&'a str>,
    pub(crate) enum_names: &'a BTreeSet<&'a str>,
    pub(crate) procedures: &'a [Procedure],
    pub(crate) webtransport: bool,
    /// `db = Postgres`: model feeds and the backplane builder method.
    pub(crate) postgres: bool,
}

pub(crate) fn generate_subscription_module(
    schema: &SubscriptionSchema<'_>,
) -> Result<proc_macro2::TokenStream, String> {
    let mut feeds = Vec::new();
    if schema.postgres {
        for model in schema.models {
            if let Some(feed) = model_feed::generate(model, schema.model_names, schema.enum_names)?
            {
                feeds.push(feed);
            }
        }
    }
    let procedures: Vec<_> = schema
        .procedures
        .iter()
        .map(procedure_feed::generate)
        .collect();
    if feeds.is_empty() && procedures.is_empty() {
        return if schema.webtransport {
            Err(
                "schema declares `extension webtransport { }`, but has no subscription op to \
                 serve over it: add `@@subscribe` to a model or declare a \
                 `subscription procedure`, or drop the extension block"
                    .to_owned(),
            )
        } else {
            Ok(proc_macro2::TokenStream::new())
        };
    }
    let model_arms = feeds.iter().map(|feed| {
        let (op_id, open) = (&feed.op_id, &feed.open_ident);
        quote! { #op_id => #open(&self.db, query, ctx).await, }
    });
    let procedure_arms = procedures.iter().map(|feed| {
        let (op_id, open) = (&feed.op_id, &feed.open_ident);
        quote! { #op_id => #open(&self.db, &self.registry, query, ctx).await, }
    });
    let functions = feeds
        .iter()
        .map(|feed| &feed.function)
        .chain(procedures.iter().map(|feed| &feed.function));
    let dispatch = dispatch::tokens(
        !procedures.is_empty(),
        schema.postgres,
        quote! { #(#model_arms)* #(#procedure_arms)* },
    );
    let webtransport_impl = schema
        .webtransport
        .then(|| webtransport_impl(!procedures.is_empty()));
    let feed_query = (!feeds.is_empty()).then(feed_query::tokens);

    Ok(quote! {
        /// The schema's subscription ops behind one transport-neutral
        /// [`::cratestack::SubscriptionDispatcher`]: the SSE endpoints open
        /// through it, and so does any other binding it is handed to.
        pub mod subscriptions {
            #dispatch

            #feed_query

            #(#functions)*

            #webtransport_impl
        }
    })
}

fn webtransport_impl(registry: bool) -> proc_macro2::TokenStream {
    let (generics, ty) = dispatch::generics(registry);
    quote! {
        impl #generics #ty {
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

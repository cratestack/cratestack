//! The generated Rust client's subscription methods
//! (`docs/design/subscriptions-websocket.md` §8): `subscribe` on an
//! `@@subscribe` model's client, and one method per `subscription
//! procedure` on `ProceduresClient`. Both resolve once the server says
//! `Ready` and hand back a `ClientSubscription<Item>`; the binding
//! (WebSocket or SSE) is the runtime's `ClientConfig`, never the method.

use cratestack_core::{Model, ModelVerb, Procedure};
use quote::quote;

use crate::procedure::procedure_client_output_item_tokens;
use crate::shared::{ident, to_snake_case};

/// The transport-specific half of a target: how the method reaches the
/// runtime, which SSE route and socket path it names, and the error type.
struct Surface {
    runtime: proc_macro2::TokenStream,
    error: proc_macro2::TokenStream,
    socket_path: &'static str,
}

fn surface(rpc: bool) -> Surface {
    if rpc {
        Surface {
            runtime: quote! { self.rpc.inner() },
            error: quote! { ::cratestack::client_rust::RpcClientError },
            socket_path: "/rpc/ws",
        }
    } else {
        Surface {
            runtime: quote! { self.runtime },
            error: quote! { ::cratestack::client_rust::ClientError },
            socket_path: "/$ws",
        }
    }
}

/// `subscribe` for an `@@subscribe` model; empty otherwise.
pub(crate) fn model_subscribe_method(model: &Model, rpc: bool) -> proc_macro2::TokenStream {
    if !cratestack_core::model_verbs(model).contains(&ModelVerb::Subscribe) {
        return proc_macro2::TokenStream::new();
    }
    let Surface {
        runtime,
        error,
        socket_path,
    } = surface(rpc);
    let op_id = ModelVerb::Subscribe.rpc_op_id(&model.name);
    let sse_path = if rpc {
        format!("/rpc/subscribe/{op_id}")
    } else {
        cratestack_core::model_subscribe_route(&model.name)
    };
    let model_ident = ident(&model.name);
    quote! {
        /// Live events of this model that the caller may read, narrowed by
        /// `input` (`list`'s filters, `operations`). Resolves once the
        /// subscription is live: nothing committed after that is missed,
        /// or the stream ends with `unavailable` and you refetch. With
        /// `fields`, records are partial: open it with
        /// `runtime.subscribe::<ModelEvent<serde_json::Value>>(..)` instead.
        pub async fn subscribe(
            &self,
            input: &::cratestack::client_rust::RpcSubscribeInput,
        ) -> Result<
            ::cratestack::client_rust::ClientSubscription<::cratestack::ModelEvent<super::models::#model_ident>>,
            #error,
        > {
            let target = ::cratestack::client_rust::SubscriptionTarget {
                op_id: #op_id.to_owned(),
                input: ::cratestack::serde_json::to_value(input)
                    .map_err(|error| ::cratestack::client_rust::ClientError::BadInput(error.to_string()))?,
                sse_path: #sse_path.to_owned(),
                sse_query: input.to_query(),
                socket_path: #socket_path,
            };
            Ok(#runtime.subscribe(target).await?)
        }
    }
}

/// The `ProceduresClient` method of one `subscription procedure`.
pub(super) fn procedure_subscribe_method(
    procedure: &Procedure,
    rpc: bool,
) -> proc_macro2::TokenStream {
    let Surface {
        runtime,
        error,
        socket_path,
    } = surface(rpc);
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let op_id = cratestack_core::subscription_procedure_op_key(procedure, true);
    let sse_path = if rpc {
        format!("/rpc/subscribe/{op_id}")
    } else {
        cratestack_core::procedure_route::procedure_rest_route_path(procedure)
    };
    let item = procedure_client_output_item_tokens(&procedure.return_type);
    let docs = format!(
        "Subscribes to `{}`. Resolves once the subscription is live; a policy or setup \
         refusal is the `Err` of this call, never a stream item.",
        procedure.name,
    );
    quote! {
        #[doc = #docs]
        pub async fn #method_ident(
            &self,
            args: &super::procedures::#module_ident::Args,
        ) -> Result<::cratestack::client_rust::ClientSubscription<#item>, #error> {
            let input = ::cratestack::serde_json::to_value(args)
                .map_err(|error| ::cratestack::client_rust::ClientError::BadInput(error.to_string()))?;
            let target = ::cratestack::client_rust::SubscriptionTarget {
                op_id: #op_id.to_owned(),
                sse_query: ::cratestack::client_rust::SubscriptionTarget::procedure_query(&input),
                input,
                sse_path: #sse_path.to_owned(),
                socket_path: #socket_path,
            };
            Ok(#runtime.subscribe(target).await?)
        }
    }
}

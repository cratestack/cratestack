//! Subscriptions in the generated package (slice S6,
//! `docs/design/subscriptions-websocket.md` §8): `src/subscriptions.ts` and
//! `src/subscription-sockets.ts` (WebTransport, WebSocket, SSE, `"auto"`),
//! emitted only for a schema with a subscription op, so every other schema
//! renders byte-for-byte as before. Under `--swr`, `src/swr/subscriptions.hooks.ts`
//! adds a generic `useSubscription` over the package's client.

use cratestack_core::{ModelVerb, Schema};

use crate::templates::{OutputPath, TemplateSpec};

/// An `@@subscribe` model or a `subscription procedure`.
pub(crate) fn has_subscriptions(schema: &Schema) -> bool {
    !schema.subscription_procedures.is_empty()
        || schema
            .models
            .iter()
            .any(|model| cratestack_core::model_verbs(model).contains(&ModelVerb::Subscribe))
}

pub(crate) const SUBSCRIPTION_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "subscriptions.ts.j2",
        output_path: OutputPath::Fixed("src/subscriptions.ts"),
        default_source: include_str!("../templates/src/subscriptions.ts.j2"),
    },
    TemplateSpec {
        template_name: "subscription-sockets.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-sockets.ts"),
        default_source: include_str!("../templates/src/subscription-sockets.ts.j2"),
    },
];

/// `--swr`: `useSubscription`, generic over any of the client's subscriptions.
pub(crate) const SWR_SUBSCRIPTION_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "swr-subscriptions-hooks.ts.j2",
    output_path: OutputPath::Fixed("src/swr/subscriptions.hooks.ts"),
    default_source: include_str!("../templates/src/swr/subscriptions-hooks.ts.j2"),
}];

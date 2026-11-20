//! Subscriptions in the generated package (slice S6,
//! `docs/design/subscriptions-websocket.md` §8): `src/subscriptions.ts` and
//! `src/subscription-end.ts` (typed end reasons, cratestack#1381) and
//! `src/subscription-sockets.ts` (WebSocket), `src/subscription-webtransport.ts` and
//! `src/subscription-socket-support.ts` (what both sockets share) and
//! `src/subscription-cbor-seq.ts` (`application/cbor-seq`, chosen not auto),
//! `src/subscription-open.ts` (the open and codec both readers share) and
//! `src/subscription-sealed.ts` (a signed open, cratestack#1378),
//! emitted only for a schema with a subscription op, so every other schema
//! renders byte-for-byte as before. Under `--swr`, `src/swr/subscriptions.hooks.ts`
//! adds a generic `useSubscription` over the package's client; under
//! `--tanstack`, `src/react-query-subscriptions.ts` adds `useSubscriptionQuery`.

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
        template_name: "subscription-end.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-end.ts"),
        default_source: include_str!("../templates/src/subscription-end.ts.j2"),
    },
    TemplateSpec {
        template_name: "subscription-sse.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-sse.ts"),
        default_source: include_str!("../templates/src/subscription-sse.ts.j2"),
    },
    TemplateSpec {
        template_name: "subscription-open.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-open.ts"),
        default_source: include_str!("../templates/src/subscription-open.ts.j2"),
    },
    TemplateSpec {
        template_name: "subscription-sealed.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-sealed.ts"),
        default_source: include_str!("../templates/src/subscription-sealed.ts.j2"),
    },
    TemplateSpec {
        template_name: "subscription-cbor-seq.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-cbor-seq.ts"),
        default_source: include_str!("../templates/src/subscription-cbor-seq.ts.j2"),
    },
    TemplateSpec {
        template_name: "subscription-socket-support.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-socket-support.ts"),
        default_source: include_str!("../templates/src/subscription-socket-support.ts.j2"),
    },
    TemplateSpec {
        template_name: "subscription-webtransport.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-webtransport.ts"),
        default_source: include_str!("../templates/src/subscription-webtransport.ts.j2"),
    },
    TemplateSpec {
        template_name: "subscription-sockets.ts.j2",
        output_path: OutputPath::Fixed("src/subscription-sockets.ts"),
        default_source: include_str!("../templates/src/subscription-sockets.ts.j2"),
    },
];

/// The structural CBOR walker `subscription-cbor-seq.ts` frames items with. An
/// RPC package already has both files (its `@stream` scanner is built on them),
/// so only a REST package with subscriptions gets them added here.
pub(crate) const REST_SUBSCRIPTION_CBOR_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "rpc-cbor-argument.ts.j2",
        output_path: OutputPath::Fixed("src/cbor-argument.ts"),
        default_source: include_str!("../templates/src/rpc-cbor-argument.ts.j2"),
    },
    TemplateSpec {
        template_name: "rpc-cbor-item.ts.j2",
        output_path: OutputPath::Fixed("src/cbor-item.ts"),
        default_source: include_str!("../templates/src/rpc-cbor-item.ts.j2"),
    },
];

/// `--tanstack`: `useSubscriptionQuery` and the `subscriptionQueryOptions`
/// behind it, generic over any of the client's subscriptions.
pub(crate) const TANSTACK_SUBSCRIPTION_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "react-query-subscriptions.ts.j2",
    output_path: OutputPath::Fixed("src/react-query-subscriptions.ts"),
    default_source: include_str!("../templates/src/react-query-subscriptions.ts.j2"),
}];

/// `--swr`: `useSubscription`, generic over any of the client's subscriptions.
pub(crate) const SWR_SUBSCRIPTION_TEMPLATE_SPECS: &[TemplateSpec] = &[TemplateSpec {
    template_name: "swr-subscriptions-hooks.ts.j2",
    output_path: OutputPath::Fixed("src/swr/subscriptions.hooks.ts"),
    default_source: include_str!("../templates/src/swr/subscriptions-hooks.ts.j2"),
}];

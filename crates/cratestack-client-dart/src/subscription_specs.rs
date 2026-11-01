//! The files a schema with a subscription op adds to the package (slice S6):
//! the socket bindings and the binding choice, and the WebTransport module
//! in its browser and stub forms (picked by a conditional import). Every
//! other schema's package is unchanged.

use cratestack_core::{ModelVerb, Schema, model_verbs};

use crate::templates::TemplateSpec;

/// An `@@subscribe` model or a `subscription procedure`.
pub(crate) fn has_subscriptions(schema: &Schema) -> bool {
    !schema.subscription_procedures.is_empty()
        || schema
            .models
            .iter()
            .any(|model| model_verbs(model).contains(&ModelVerb::Subscribe))
}

pub(crate) const SUBSCRIPTION_TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "subscription-sockets.dart.j2",
        output_path: "lib/src/subscription_sockets.dart",
        default_source: include_str!("../templates/subscription-sockets.dart.j2"),
    },
    TemplateSpec {
        template_name: "subscription-webtransport-web.dart.j2",
        output_path: "lib/src/subscription_webtransport_web.dart",
        default_source: include_str!("../templates/subscription-webtransport-web.dart.j2"),
    },
    TemplateSpec {
        template_name: "subscription-webtransport-stub.dart.j2",
        output_path: "lib/src/subscription_webtransport_stub.dart",
        default_source: include_str!("../templates/subscription-webtransport-stub.dart.j2"),
    },
];

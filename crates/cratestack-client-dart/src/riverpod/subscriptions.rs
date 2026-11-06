//! Subscriptions in the riverpod preset (slice S6,
//! `docs/design/subscriptions-websocket.md`): the stream providers' names,
//! and the one predicate every file's gates share.
use std::collections::BTreeSet;

use cratestack_core::{Model, ModelVerb, Schema, model_verbs};

use crate::idents::to_camel_case;
use crate::riverpod::provider_naming::reserve_operation_symbol;

/// Whether the schema has any subscription op: the client's `subscribe`.
pub(crate) fn has_subscriptions(schema: &Schema) -> bool {
    crate::subscription_specs::has_subscriptions(schema)
}

pub(crate) fn allows_subscribe(model: &Model) -> bool {
    model_verbs(model).contains(&ModelVerb::Subscribe)
}

/// `<model>Events`, reserved after the model's other five providers so no
/// existing name moves; `None` for a model without `@@subscribe`.
pub(crate) fn model_subscribe_symbol(
    model: &Model,
    provider_prefix: &str,
    occupied: &mut BTreeSet<String>,
) -> Option<String> {
    allows_subscribe(model).then(|| {
        reserve_operation_symbol(
            &format!("{}Events", to_camel_case(&model.name)),
            false,
            provider_prefix,
            occupied,
        )
    })
}

/// One stream provider per subscription procedure, named like a query
/// procedure's (its method name), reserved after every unary one.
pub(crate) fn subscription_procedure_symbols<'a>(
    method_names: impl IntoIterator<Item = &'a str>,
    provider_prefix: &str,
    occupied: &mut BTreeSet<String>,
) -> Vec<String> {
    method_names
        .into_iter()
        .map(|name| reserve_operation_symbol(name, false, provider_prefix, occupied))
        .collect()
}

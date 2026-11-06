//! A model feed's input (`docs/design/subscriptions-websocket.md` §3.1, §5.3,
//! slice S3), whichever binding carried it: the REST query of
//! `GET /<plural>/$subscribe`, or an `RpcSubscribeInput` (RPC `POST`,
//! WebTransport and WebSocket request frames), which becomes the same query.
//!
//! `operations` is split out here; the rest is `list`'s filter syntax, parsed
//! by the generated `list` parser so a feed filters exactly as a read does.
//! `list`'s paging, ordering and inclusion parameters mean nothing for a
//! stream of single events and are refused rather than ignored.

use cratestack_core::rpc::RpcSubscribeInput;
use cratestack_core::{CratestackError, ModelEventKind, SubscriptionStream};
use futures_util::StreamExt;

/// The operations a feed wants (`None`: all the model emits) and the rest of
/// its query, for the `list` parser.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SubscribeQuery {
    pub operations: Option<Vec<ModelEventKind>>,
    pub rest: Option<String>,
}

const LIST_ONLY: &[&str] = &[
    "limit",
    "offset",
    "sort",
    "orderBy",
    "include",
    "computedParams",
];

pub fn split_subscribe_query(raw: Option<&str>) -> Result<SubscribeQuery, CratestackError> {
    let mut operations = None;
    let mut rest = Vec::new();
    for (key, value) in crate::parse_query_pairs(raw)? {
        if key == "operations" {
            let parsed = value
                .split(',')
                .map(str::trim)
                .filter(|op| !op.is_empty())
                .map(|op| {
                    ModelEventKind::parse(op).map_err(|_| {
                        CratestackError::BadRequest(format!(
                            "unknown operation `{op}`: expected created, updated or deleted"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            operations = Some(parsed);
        } else if LIST_ONLY.contains(&key.as_str()) || key.starts_with("includeFields[") {
            return Err(CratestackError::BadRequest(format!(
                "`{key}` does not apply to a subscription"
            )));
        } else {
            rest.push((key, value));
        }
    }
    let rest = (!rest.is_empty()).then(|| {
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(rest.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish()
    });
    Ok(SubscribeQuery { operations, rest })
}

/// A request frame's input as the REST query: `null` or `{}` is none.
pub fn subscribe_input_query(input: serde_json::Value) -> Result<Option<String>, CratestackError> {
    if input.is_null() {
        return Ok(None);
    }
    let input: RpcSubscribeInput = serde_json::from_value(input).map_err(|error| {
        CratestackError::BadRequest(format!("invalid subscription input: {error}"))
    })?;
    Ok(input.to_query())
}

/// `items` with each event's record narrowed to `fields` (`None`: all).
pub fn project_event_fields(
    items: SubscriptionStream<serde_json::Value>,
    fields: Option<Vec<String>>,
) -> SubscriptionStream<serde_json::Value> {
    let Some(fields) = fields else {
        return items;
    };
    Box::pin(items.map(move |item| {
        item.map(|mut event| {
            if let Some(serde_json::Value::Object(data)) = event.get_mut("data") {
                data.retain(|key, _| fields.iter().any(|field| field == key));
            }
            event
        })
    }))
}

#[cfg(test)]
mod tests;

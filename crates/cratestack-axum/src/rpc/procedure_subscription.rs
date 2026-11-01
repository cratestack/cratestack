//! A `subscription procedure`'s input and items
//! (`docs/design/subscriptions-websocket.md` §3.2, §3.3, slice S4).
//!
//! The input is the procedure's `Args` object, keyed by argument name, the
//! same object a unary procedure takes over RPC. A binding that carries a
//! JSON input (WebTransport, `POST /rpc/subscribe/{op_id}`) hands it over
//! as is; `GET` carries each argument as a query parameter whose value is
//! that argument's JSON (`?args={"roomId":"r1"}`), so an `EventSource` can
//! open one. Both meet as the query string, the form the generated
//! dispatcher's `open_query` takes.
//!
//! A handler's stream ends the subscription when it ends: unlike a model
//! feed, it is not a backplane that can drop events, so a natural end is
//! not an error. An `Err` item is the stream's terminal error.

use cratestack_core::{CratestackError, SubscriptionStream};
use futures_util::stream::{self, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A subscription procedure's JSON input as the query string `open_query`
/// takes: `null` or `{}` is none.
pub fn procedure_input_query(input: serde_json::Value) -> Result<Option<String>, CratestackError> {
    let object = match input {
        serde_json::Value::Null => return Ok(None),
        serde_json::Value::Object(object) => object,
        _ => {
            return Err(CratestackError::BadRequest(
                "a subscription procedure's input is an object of its arguments".to_owned(),
            ));
        }
    };
    if object.is_empty() {
        return Ok(None);
    }
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in &object {
        query.append_pair(name, &value.to_string());
    }
    Ok(Some(query.finish()))
}

/// The procedure's `Args` from the query string: each parameter is one
/// argument, its value that argument's JSON.
pub fn procedure_query_args<A: DeserializeOwned>(
    query: Option<&str>,
) -> Result<A, CratestackError> {
    let mut object = serde_json::Map::new();
    for (name, value) in crate::parse_query_pairs(query)? {
        let value = serde_json::from_str(&value).map_err(|_| {
            CratestackError::BadRequest(format!(
                "argument `{name}` must be JSON, such as `{name}={{\"id\":1}}` or `{name}=\"text\"`"
            ))
        })?;
        if object.insert(name.clone(), value).is_some() {
            return Err(CratestackError::BadRequest(format!(
                "argument `{name}` is given twice"
            )));
        }
    }
    serde_json::from_value(serde_json::Value::Object(object)).map_err(|error| {
        CratestackError::BadRequest(format!("invalid subscription arguments: {error}"))
    })
}

/// A handler's items encoded to JSON values, ending after the first error.
pub fn procedure_feed<T>(items: SubscriptionStream<T>) -> SubscriptionStream<serde_json::Value>
where
    T: Serialize + Send + 'static,
{
    Box::pin(stream::unfold(Some(items), |items| async move {
        let mut items = items?;
        let item = match items.next().await? {
            Ok(item) => serde_json::to_value(&item).map_err(|error| {
                CratestackError::Codec(format!("failed to encode a subscription item: {error}"))
            }),
            Err(error) => Err(error),
        };
        let rest = item.is_ok().then_some(items);
        Some((item, rest))
    }))
}

#[cfg(test)]
mod tests;

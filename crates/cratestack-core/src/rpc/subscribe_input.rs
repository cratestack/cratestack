//! RPC input for `model.<X>.subscribe` (`docs/design/subscriptions-websocket.md`
//! §3.1): which events of the model a subscription wants. It mirrors the REST
//! `GET /<plural>/$subscribe` query one to one, so every binding filters with
//! the `list` filter syntax and the same parser (RPC re-enters it through the
//! query string, the transport-parity mechanism `RpcListInput` uses).

use serde::{Deserialize, Serialize};

use super::RpcListPredicate;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcSubscribeInput {
    /// Filter expression, `list`'s `?where=` syntax.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "where")]
    pub where_expr: Option<String>,
    /// Disjunction filter (`?or=`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub or: Option<String>,
    /// Field predicates (`?status=open`, `?amount__gt=10`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<RpcListPredicate>,
    /// Projection of each event's record (`?fields=id,status`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    /// The operations wanted, of those the model emits (`?operations=created,deleted`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<String>>,
}

impl RpcSubscribeInput {
    /// The equivalent REST query string; `None` when there is nothing to say.
    pub fn to_query(&self) -> Option<String> {
        let mut pairs: Vec<(String, String)> = Vec::new();
        if let Some(where_expr) = &self.where_expr {
            pairs.push(("where".to_owned(), where_expr.clone()));
        }
        if let Some(or) = &self.or {
            pairs.push(("or".to_owned(), or.clone()));
        }
        for predicate in &self.filters {
            pairs.push((predicate.key.clone(), predicate.value.clone()));
        }
        if let Some(fields) = &self.fields {
            pairs.push(("fields".to_owned(), fields.join(",")));
        }
        if let Some(operations) = &self.operations {
            pairs.push(("operations".to_owned(), operations.join(",")));
        }
        if pairs.is_empty() {
            return None;
        }
        Some(
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .finish(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{RpcListPredicate, RpcSubscribeInput};

    #[test]
    fn an_input_becomes_the_rest_query() {
        let input = RpcSubscribeInput {
            filters: vec![RpcListPredicate {
                key: "status".to_owned(),
                value: "open".to_owned(),
            }],
            fields: Some(vec!["id".to_owned(), "status".to_owned()]),
            operations: Some(vec!["created".to_owned()]),
            ..Default::default()
        };
        assert_eq!(
            input.to_query().as_deref(),
            Some("status=open&fields=id%2Cstatus&operations=created")
        );
        assert_eq!(RpcSubscribeInput::default().to_query(), None);
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let error = serde_json::from_value::<RpcSubscribeInput>(serde_json::json!({"wher": "x"}));
        assert!(error.is_err());
    }
}

//! Caller identity captured by the auth provider.
//!
//! [`CoolAuthIdentity`] is the legacy flat-claims shape kept for the
//! generated route handlers; modern callers reach for
//! [`super::principal::PrincipalContext`] which surfaces the same
//! claims plus structured actor / session / tenant facets.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::CoolError;
use crate::value::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoolAuthIdentity {
    pub fields: BTreeMap<String, Value>,
}

impl CoolAuthIdentity {
    pub fn from_principal<P: Serialize>(principal: P) -> Result<Self, CoolError> {
        let value = serde_json::to_value(principal).map_err(|error| {
            CoolError::Internal(format!("failed to serialize auth principal: {error}"))
        })?;
        let serde_json::Value::Object(object) = value else {
            return Err(CoolError::Internal(
                "auth principal must serialize to a JSON object".to_owned(),
            ));
        };

        let mut fields = BTreeMap::new();
        for (key, value) in object {
            fields.insert(key, json_value_to_cool_value(value)?);
        }

        Ok(Self { fields })
    }
}

/// Convert a `serde_json::Value` claim into a framework-native
/// [`Value`]. Shared by the principal/identity builders.
pub(super) fn json_value_to_cool_value(value: serde_json::Value) -> Result<Value, CoolError> {
    match value {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(value) => Ok(Value::Bool(value)),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Ok(Value::Int(value))
            } else if let Some(value) = number.as_f64() {
                Ok(Value::Float(value))
            } else {
                Err(CoolError::Internal(format!(
                    "unsupported auth principal number '{number}'"
                )))
            }
        }
        serde_json::Value::String(value) => Ok(Value::String(value)),
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(json_value_to_cool_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        serde_json::Value::Object(object) => object
            .into_iter()
            .map(|(key, value)| json_value_to_cool_value(value).map(|value| (key, value)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(Value::Map),
    }
}

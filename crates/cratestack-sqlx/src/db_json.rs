use serde::{Deserialize, Deserializer, Serialize, Serializer};

use cratestack_core::Value;

/// Wrapper used solely for SQL persistence: serializes `Value` as plain
/// JSON (via serde_json::Value) and deserializes plain JSON back into
/// `Value`. This keeps the internal `Value` serde representation
/// (external tagging) unchanged for the rest of the framework.
#[derive(Debug, Clone, PartialEq)]
pub struct DbJson(pub Value);

impl From<Value> for DbJson {
    fn from(v: Value) -> Self {
        DbJson(v)
    }
}

impl From<DbJson> for Value {
    fn from(d: DbJson) -> Self {
        d.0
    }
}

impl Serialize for DbJson {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let json = self.0.clone().into_json();
        json.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DbJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = serde_json::Value::deserialize(deserializer)?;
        Ok(DbJson(Value::from_json(json)))
    }
}

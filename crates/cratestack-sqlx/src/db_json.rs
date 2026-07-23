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

#[cfg(test)]
mod tests {
    use super::DbJson;
    use cratestack_core::Value;
    use serde_json;
    use std::collections::BTreeMap;

    #[test]
    fn serialize_empty_map_as_object() {
        let map: BTreeMap<String, Value> = BTreeMap::new();
        let v = Value::Map(map);
        let db = DbJson(v);
        let s = serde_json::to_string(&db).expect("serialize failed");
        assert_eq!(s, "{}");
    }

    #[test]
    fn serialize_list_of_string_as_array() {
        let list = Value::List(vec![Value::String("x".to_string())]);
        let db = DbJson(list);
        let s = serde_json::to_string(&db).expect("serialize failed");
        assert_eq!(s, "[\"x\"]");
    }

    #[test]
    fn deserialize_plain_json_object_to_value_map() {
        let s = "{\"a\": 1}";
        let db: DbJson = serde_json::from_str(s).expect("deserialize failed");
        match db.0 {
            Value::Map(m) => {
                assert!(m.contains_key("a"));
            }
            _ => panic!("expected map"),
        }
    }
}

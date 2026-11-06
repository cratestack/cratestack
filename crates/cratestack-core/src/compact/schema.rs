//! Tables for every op of a schema at its current contract.

use super::tables::KeyTables;
use crate::client_contract::{op_contract_digest, op_contract_json, op_keys};
use crate::schema::Schema;

impl KeyTables {
    /// Tables for every op `schema` exposes, each at its current contract
    /// digest (the ops [`op_keys`] lists, in that order).
    ///
    /// ```
    /// let schema: cratestack_core::Schema = serde_json::from_str(
    ///     r#"{"datasource":null,"auth":null,"config_blocks":[],"mixins":[],
    ///         "models":[],"types":[],"enums":[],"procedures":[]}"#,
    /// )
    /// .unwrap();
    /// assert!(cratestack_core::compact::KeyTables::from_schema(&schema).ops.is_empty());
    /// ```
    pub fn from_schema(schema: &Schema) -> KeyTables {
        let mut tables = KeyTables::default();
        for key in op_keys(schema) {
            let (Some(json), Some(digest)) = (
                op_contract_json(schema, &key),
                op_contract_digest(schema, &key),
            ) else {
                continue;
            };
            let contract = serde_json::from_str(&json)
                .expect("an op contract is canonical JSON, so it parses");
            tables
                .add_contract(digest, &contract)
                .expect("a contract this build derived is always readable");
        }
        tables
    }
}

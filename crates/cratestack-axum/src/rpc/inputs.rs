//! RPC input shapes.
//!
//! The RPC binding wraps each model verb's input in a stable, model-agnostic
//! shape. The macro decodes the body into one of these, then reconstructs
//! whatever axum extractor the existing CRUD handler expects (`Path(id)`,
//! `RawQuery(...)`, `Bytes`) and delegates. The handlers themselves are
//! untouched.
//!
//! The list shape mirrors the REST URL query 1:1 — same keys, same semantics —
//! so REST clients can migrate to RPC without re-learning the filter / order /
//! pagination vocabulary. Synthesis back to a URL query happens in
//! [`super::synthesize_list_query`]; the existing list handler parses it via
//! `parse_model_list_query`.

use serde::{Deserialize, Serialize};

/// RPC input for `model.<X>.get` and `model.<X>.delete`. The PK type is
/// instantiated per-model at the macro emission site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPkInput<Pk> {
    pub id: Pk,
}

/// RPC input for `model.<X>.update`. Parameterized on both the PK type
/// and the model's concrete `Update<X>Input` so the patch decodes
/// straight to its real type — round-tripping through
/// `serde_json::Value` would corrupt CBOR `Option::None` values (which
/// `minicbor-serde` encodes as `0xf6` simple-null but `serde_json::Value`
/// encodes as the CBOR empty-array marker; see comments in
/// `cratestack-codec-cbor`). The dispatcher re-encodes `patch` through
/// the same codec before handing it to the existing update handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcUpdateInput<Pk, Patch> {
    pub id: Pk,
    pub patch: Patch,
}

/// Single arbitrary key/value predicate inside [`RpcListInput::filters`].
/// Models the REST URL form's "anything that isn't a reserved keyword is a
/// predicate" rule (e.g. `?published=true&authorId=42`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcListPredicate {
    pub key: String,
    pub value: String,
}

/// RPC input for `model.<X>.list`. Mirrors the REST URL query 1:1 — every
/// optional field maps to a query param of the same name, predicates carry
/// arbitrary `(key, value)` pairs that aren't reserved keywords.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RpcListInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Selection fields (`?fields=a,b,c`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    /// Included relations (`?include=author,comments`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Fields per included relation (`?includeFields[author]=id,name`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub include_fields: std::collections::BTreeMap<String, Vec<String>>,
    /// Order expression (`?sort=name asc`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    /// Top-level filter expression (`?where=...`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "where")]
    pub where_expr: Option<String>,
    /// Disjunction filter (`?or=...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub or: Option<String>,
    /// Arbitrary `key=value` predicates (anything not in the reserved set).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<RpcListPredicate>,
}

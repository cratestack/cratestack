//! RPC model-CRUD input envelopes (cratestack#490).
//!
//! Moved here from `cratestack-axum::rpc::inputs`, which is where they lived
//! until this fix — an oversight relative to the parent module's opening
//! paragraph ("They live in `cratestack-core` so clients can depend on a
//! single source of truth without pulling in axum") and relative to the
//! sibling wire shapes in `super` (`RpcErrorBody`/`RpcRequest`/`RpcResponseFrame`),
//! which already made that move. It went unnoticed until `cratestack-client`
//! (a facade with genuinely no `cratestack-axum` dependency) tried to compile
//! a `transport rpc` schema with model CRUD and hit "cannot find
//! `RpcListInput` in `rpc`" — every previous consumer of
//! `::cratestack::rpc::RpcListInput` (`cratestack-pg`, `cratestack-api`,
//! `cratestack-sqlite`) carries `cratestack-axum` regardless, so the wrong
//! source crate was invisible until a facade without it existed to surface
//! it. `cratestack-axum::rpc` re-exports these same three types verbatim
//! (`pub use cratestack_core::rpc::{RpcListInput, RpcListPredicate,
//! RpcPkInput, RpcUpdateInput};`), so `cratestack-pg`/`cratestack-api`/
//! `cratestack-sqlite` see no behavior change — same names, same shapes, same
//! wire format, only the defining crate moved.
//! ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
/// RPC input for `model.<X>.delete`. The PK type is instantiated
/// per-model at the macro emission site.
///
/// `model.<X>.get` uses [`RpcGetInput`] instead, not this type — see that
/// type's doc comment for why the two aren't shared.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPkInput<Pk> {
    pub id: Pk,
}

/// RPC input for `model.<X>.get`. Deliberately its own struct rather than
/// reusing [`RpcPkInput`] for both get and delete: `delete` also decodes
/// `RpcPkInput`, and adding `computedParams` there would be a silently-
/// ignored field on a verb that has no response body to carry resolved
/// values into.
///
/// An old `{"id": 1}` frame (no `computedParams` key) decodes unchanged
/// under `#[serde(default)]`, and a new client that leaves
/// `computed_params` unset emits a byte-identical frame — this is the
/// additive-`#[serde(default)]` shape `docs/design/rpc-transport.md` §7
/// blesses as not requiring a snapshot-format-version bump.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcGetInput<Pk> {
    pub id: Pk,
    /// Raw JSON-object text, same field and semantics as
    /// [`RpcListInput::computed_params`] — see that field's doc comment
    /// for why this carries a `String` rather than `serde_json::Value`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "computedParams"
    )]
    pub computed_params: Option<String>,
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
    /// Raw JSON-object text of `?computedParams=` (`{"<field>": {...}}`),
    /// keyed by computed field name. Carried as the RAW JSON-object text,
    /// NOT `serde_json::Value`, for three reasons: (a) this file's own
    /// docs above note that round-tripping an optional-bearing value
    /// through `serde_json::Value` corrupts CBOR `Option::None` —
    /// generated params types are bags of optionals, so they'd hit that
    /// head-on; (b) `/rpc/batch` re-encodes `RpcRequest::input` through
    /// `serde_json::Value` (`cratestack-axum::rpc::batch`) and a `String`
    /// survives that round trip verbatim, a nested object wouldn't; (c)
    /// carrying the same bytes REST puts on `?computedParams=` means the
    /// SAME `parse_<model>_computed_params` validation runs unmodified —
    /// one implementation, no drift between transports.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "computedParams"
    )]
    pub computed_params: Option<String>,
}

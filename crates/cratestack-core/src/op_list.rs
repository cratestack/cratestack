//! The one list of ops a schema exposes (cratestack#1123, EXT-14).
//!
//! Which verbs a model has, and how an op is keyed, used to be derived
//! separately by the RPC descriptors, the REST descriptors, the axum router,
//! RPC dispatch and every client generator, each by asking
//! [`model_internal_actions`](crate::model_internal_actions) and adding its
//! own list of verb names. A contract digest is keyed by op, so those copies
//! must not be able to disagree. They all ask [`model_verbs`] now, and
//! [`crate::client_contract`]'s op list is built from it.
//!
//! A *generator* may still expose fewer verbs than this list (the TypeScript
//! client omits `create` for a model with no create policy): that is a
//! client-side choice on top of this list, never a verb beyond it.

use crate::procedure_route::procedure_rest_route_path;
use crate::route_naming::model_route_segment;
use crate::schema::{Model, Procedure, model_internal_actions};

/// What an op does to its model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelVerb {
    List,
    Get,
    Create,
    Update,
    Delete,
    /// Only for `@@subscribe` models, and only under `transport rpc`.
    Subscribe,
}

impl ModelVerb {
    /// The five CRUD verbs, in the order every surface lists them.
    pub const CRUD: [ModelVerb; 5] = [
        Self::List,
        Self::Get,
        Self::Create,
        Self::Update,
        Self::Delete,
    ];

    /// The wire spelling: the last segment of an RPC op id, and the
    /// vocabulary `@@internal(...)` speaks.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Get => "get",
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Subscribe => "subscribe",
        }
    }

    /// `(REST method, is it the detail route?)`; `None` for `subscribe`,
    /// which only exists under `transport rpc`.
    pub const fn rest(self) -> Option<(&'static str, bool)> {
        Some(match self {
            Self::List => ("GET", false),
            Self::Create => ("POST", false),
            Self::Get => ("GET", true),
            Self::Update => ("PATCH", true),
            Self::Delete => ("DELETE", true),
            Self::Subscribe => return None,
        })
    }

    /// `model.<Model>.<verb>`.
    pub fn rpc_op_id(self, model: &str) -> String {
        format!("model.{model}.{}", self.as_str())
    }
}

/// The verbs `model` exposes, in [`ModelVerb::CRUD`] order with `Subscribe`
/// last: every CRUD verb `@@internal` does not suppress, plus `Subscribe`
/// for an `@@subscribe` model.
pub fn model_verbs(model: &Model) -> Vec<ModelVerb> {
    let internal = model_internal_actions(model);
    let mut verbs: Vec<ModelVerb> = ModelVerb::CRUD
        .into_iter()
        .filter(|verb| !internal.contains(verb.as_str()))
        .collect();
    if model.attributes.iter().any(|a| a.raw == "@@subscribe") {
        verbs.push(ModelVerb::Subscribe);
    }
    verbs
}

/// The collection route of a model on REST: `/widgets`.
pub fn model_list_route(model: &str) -> String {
    format!("/{}", model_route_segment(model))
}

/// The detail route of a model on REST: `/widgets/{id}`.
pub fn model_detail_route(model: &str) -> String {
    format!("{}/{{id}}", model_list_route(model))
}

/// The op key of a model verb: the RPC op id, or `"<METHOD> <route>"` on
/// REST. `None` for a verb REST does not have.
pub fn model_op_key(model: &str, verb: ModelVerb, rpc: bool) -> Option<String> {
    if rpc {
        return Some(verb.rpc_op_id(model));
    }
    let (method, detail) = verb.rest()?;
    let route = if detail {
        model_detail_route(model)
    } else {
        model_list_route(model)
    };
    Some(format!("{method} {route}"))
}

/// The op key of a procedure: `procedure.<name>` on RPC, `POST <path>` on
/// REST (the path carries `@api_version`).
pub fn procedure_op_key(procedure: &Procedure, rpc: bool) -> String {
    if rpc {
        format!("procedure.{}", procedure.name)
    } else {
        format!("POST {}", procedure_rest_route_path(procedure))
    }
}

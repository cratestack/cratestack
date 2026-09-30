//! The ops a schema exposes to a client, keyed the way the generated
//! routers and clients key them: the RPC `op_id`, or `"<METHOD> <route>"`
//! on REST. One list, asserted equal to the macro's `OPS` and
//! `ROUTE_TRANSPORTS` by `cratestack-pg/tests/op_contract_parity.rs`.

use crate::procedure_route::procedure_rest_route_path;
use crate::route_naming::model_route_segment;
use crate::schema::{Model, Procedure, Schema, TransportStyle, model_internal_actions};

/// What an op does to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelVerb {
    List,
    Get,
    Create,
    Update,
    Delete,
    Subscribe,
}

impl ModelVerb {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Get => "get",
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Subscribe => "subscribe",
        }
    }

    /// `(REST method, detail route?)`; `None` for `subscribe`, which only
    /// exists under `transport rpc`.
    fn rest(self) -> Option<(&'static str, bool)> {
        Some(match self {
            Self::List => ("GET", false),
            Self::Create => ("POST", false),
            Self::Get => ("GET", true),
            Self::Update => ("PATCH", true),
            Self::Delete => ("DELETE", true),
            Self::Subscribe => return None,
        })
    }
}

/// The declaration an op is about.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OpTarget<'a> {
    Model(&'a Model, ModelVerb),
    Procedure(&'a Procedure),
}

/// One client-facing op.
#[derive(Debug, Clone)]
pub(crate) struct ClientOp<'a> {
    pub key: String,
    pub target: OpTarget<'a>,
}

const VERBS: [ModelVerb; 5] = [
    ModelVerb::List,
    ModelVerb::Get,
    ModelVerb::Create,
    ModelVerb::Update,
    ModelVerb::Delete,
];

/// Every op `schema` exposes, sorted by key. `@@internal` verbs are
/// absent; `subscribe` is present for `@@subscribe` models.
pub(crate) fn ops(schema: &Schema) -> Vec<ClientOp<'_>> {
    let rpc = schema.transport == TransportStyle::Rpc;
    let mut out = Vec::new();
    for model in &schema.models {
        let internal = model_internal_actions(model);
        let mut verbs: Vec<ModelVerb> = VERBS
            .into_iter()
            .filter(|verb| !internal.contains(verb.as_str()))
            .collect();
        if model.attributes.iter().any(|a| a.raw == "@@subscribe") {
            verbs.push(ModelVerb::Subscribe);
        }
        for verb in verbs {
            if let Some(key) = model_key(&model.name, verb, rpc) {
                out.push(ClientOp {
                    key,
                    target: OpTarget::Model(model, verb),
                });
            }
        }
    }
    for procedure in &schema.procedures {
        let key = if rpc {
            format!("procedure.{}", procedure.name)
        } else {
            format!("POST {}", procedure_rest_route_path(procedure))
        };
        out.push(ClientOp {
            key,
            target: OpTarget::Procedure(procedure),
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

fn model_key(model: &str, verb: ModelVerb, rpc: bool) -> Option<String> {
    if rpc {
        return Some(format!("model.{model}.{}", verb.as_str()));
    }
    let (method, detail) = verb.rest()?;
    let segment = model_route_segment(model);
    let path = if detail {
        format!("/{segment}/{{id}}")
    } else {
        format!("/{segment}")
    };
    Some(format!("{method} {path}"))
}

/// The key of every op `schema` exposes, sorted. This is the list the
/// generated routers and clients expose, asserted equal to the macro's
/// `OPS` / `ROUTE_TRANSPORTS` by the parity test.
pub fn op_keys(schema: &Schema) -> Vec<String> {
    ops(schema).into_iter().map(|op| op.key).collect()
}

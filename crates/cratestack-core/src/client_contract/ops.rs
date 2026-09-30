//! The ops a schema exposes to a client, keyed the way the generated
//! routers and clients key them: the RPC `op_id`, or `"<METHOD> <route>"`
//! on REST. Built from [`crate::op_list`], the one list every surface
//! consumes, and asserted equal to the macro's `OPS` and `ROUTE_TRANSPORTS`
//! by `cratestack-pg/tests/op_contract_parity.rs`.

use crate::op_list::{ModelVerb, model_op_key, model_verbs, procedure_op_key};
use crate::schema::{Model, Procedure, Schema, TransportStyle};

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

/// Every op `schema` exposes, sorted by key. `@@internal` verbs are
/// absent; `subscribe` is present for `@@subscribe` models.
pub(crate) fn ops(schema: &Schema) -> Vec<ClientOp<'_>> {
    let rpc = schema.transport == TransportStyle::Rpc;
    let mut out = Vec::new();
    for model in &schema.models {
        for verb in model_verbs(model) {
            if let Some(key) = model_op_key(&model.name, verb, rpc) {
                out.push(ClientOp {
                    key,
                    target: OpTarget::Model(model, verb),
                });
            }
        }
    }
    for procedure in &schema.procedures {
        out.push(ClientOp {
            key: procedure_op_key(procedure, rpc),
            target: OpTarget::Procedure(procedure),
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// The key of every op `schema` exposes, sorted: the keys the generated
/// routers and clients expose, asserted equal to the macro's `OPS` /
/// `ROUTE_TRANSPORTS` by the parity test.
pub fn op_keys(schema: &Schema) -> Vec<String> {
    ops(schema).into_iter().map(|op| op.key).collect()
}

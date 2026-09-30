//! Builds an op's canonical contract (see the parent module).

use super::canon::{COpContract, CWireProcedure};
use super::ops::{ClientOp, ModelVerb, OpTarget};
use super::project;
use crate::events::{ModelEventKind, parse_emit_attribute};
use crate::schema::{Procedure, ProcedureKind, Schema, TransportStyle, TypeArity};
use crate::schema_identity::members::{args, type_ref};

pub(super) fn contract<'a>(schema: &'a Schema, op: &'a ClientOp<'a>) -> COpContract<'a> {
    let transport = match schema.transport {
        TransportStyle::Rpc => "rpc",
        TransportStyle::Rest => "rest",
    };
    match op.target {
        OpTarget::Model(model, verb) => COpContract {
            closure: project::closure(schema, &[&model.name]),
            events: (verb == ModelVerb::Subscribe).then(|| emitted(model)),
            key: &op.key,
            kind: if verb == ModelVerb::Subscribe {
                "subscription"
            } else {
                "unary"
            },
            model: Some(&model.name),
            procedure: None,
            transport,
            verb: verb.as_str(),
        },
        OpTarget::Procedure(p) => procedure_contract(schema, op, p, transport),
    }
}

fn procedure_contract<'a>(
    schema: &'a Schema,
    op: &'a ClientOp<'a>,
    p: &'a Procedure,
    transport: &'static str,
) -> COpContract<'a> {
    let Procedure {
        docs: _,
        name,
        name_span: _,
        kind,
        args: procedure_args,
        return_type,
        attributes,
        span: _,
        mcp: _,
    } = p;
    let mut roots: Vec<&str> = Vec::new();
    project::type_names(return_type, &mut roots);
    for arg in procedure_args {
        project::type_names(&arg.ty, &mut roots);
    }
    let kind = match kind {
        ProcedureKind::Query => "query",
        ProcedureKind::Mutation => "mutation",
    };
    COpContract {
        closure: project::closure(schema, &roots),
        events: None,
        key: &op.key,
        kind: if return_type.arity == TypeArity::List {
            "sequence"
        } else {
            "unary"
        },
        model: None,
        procedure: Some(CWireProcedure {
            args: args(procedure_args),
            attributes: project::wire_attributes(attributes),
            kind,
            name,
            return_type: type_ref(return_type),
        }),
        transport,
        verb: kind,
    }
}

fn emitted(model: &crate::schema::Model) -> Vec<&'static str> {
    let mut kinds: Vec<ModelEventKind> = model
        .attributes
        .iter()
        .filter(|a| a.raw.starts_with("@@emit("))
        .filter_map(|a| parse_emit_attribute(&a.raw).ok())
        .flatten()
        .collect();
    kinds.sort_by_key(|k| k.as_str());
    kinds.dedup();
    kinds.into_iter().map(ModelEventKind::as_str).collect()
}

pub(super) fn canonical(schema: &Schema, op: &ClientOp<'_>) -> Vec<u8> {
    serde_json::to_vec(&contract(schema, op)).expect("plain structs always serialize")
}

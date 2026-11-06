//! The request and response root of an op, read off its contract.
//!
//! The roots follow what the generated handlers really put on the wire:
//!
//! | op | request | response |
//! |---|---|---|
//! | model `create`, `update` | the model's struct (the `Create`/`Update` input carries a subset of its fields) | the model |
//! | model `get`, `delete` | opaque (no body) | the model |
//! | model `list` | opaque (`RpcListInput` and query strings stay named) | `Page(model)` for `@@paged`, else `List(model)` |
//! | model `subscribe` | opaque | opaque (event frames are framework-owned) |
//! | procedure | the generated `Args` wrapper, numbered by `procedure.args` | the return type |
//! | `@stream` or subscription procedure | as above, opaque for a subscription (arguments travel in the query) | one stream item: the return type with its list arity removed |

use serde_json::Value;

use super::derive::{Closure, array_of, bad, str_of};
use super::error::CompactError;
use super::tables::{KeyNode, KeyStruct, KeyTables};

/// `(request, response)` for the op `contract` describes, its closure
/// interned at `base`.
pub(super) fn roots(
    tables: &mut KeyTables,
    contract: &Value,
    closure: &Closure<'_>,
    base: u32,
) -> Result<(KeyNode, KeyNode), CompactError> {
    match contract.get("model").and_then(Value::as_str) {
        Some(model) => model_roots(contract, closure, model, base),
        None => procedure_roots(tables, contract, closure, base),
    }
}

fn model_roots(
    contract: &Value,
    closure: &Closure<'_>,
    model: &str,
    base: u32,
) -> Result<(KeyNode, KeyNode), CompactError> {
    let Some(&index) = closure.names.get(model) else {
        return Err(bad(format!("model `{model}` is not in its own closure")));
    };
    let node = KeyNode::Struct(index + base);
    let paged = closure
        .decl(model)
        .and_then(|decl| decl.get("attributes"))
        .and_then(Value::as_array)
        .is_some_and(|attrs| attrs.iter().any(|a| attribute_name(a) == "@@paged"));
    Ok(match str_of(contract, "verb")? {
        "create" | "update" => (node.clone(), node),
        "get" | "delete" => (KeyNode::Opaque, node),
        "list" if paged => (KeyNode::Opaque, KeyNode::page(node)),
        "list" => (KeyNode::Opaque, KeyNode::list(node)),
        _ => (KeyNode::Opaque, KeyNode::Opaque),
    })
}

/// Absolute nodes again: the wrapper's fields are shifted by `base` before
/// it is interned, since it lives beside the closure's run, not inside it.
fn procedure_roots(
    tables: &mut KeyTables,
    contract: &Value,
    closure: &Closure<'_>,
    base: u32,
) -> Result<(KeyNode, KeyNode), CompactError> {
    let procedure = contract
        .get("procedure")
        .ok_or_else(|| bad("an op without `model` has no `procedure`"))?;
    let returns = procedure
        .get("return_type")
        .ok_or_else(|| bad("procedure without `return_type`"))?;
    let subscription = str_of(contract, "kind")? == "subscription";
    let streamed = subscription
        || procedure
            .get("attributes")
            .and_then(Value::as_array)
            .is_some_and(|attrs| attrs.iter().any(|a| attribute_name(a) == "@stream"));
    let response = if streamed {
        closure.element(returns)
    } else {
        closure.node(returns)
    }
    .shifted(base);
    let args = array_of(procedure, "args")?;
    if subscription || args.is_empty() {
        return Ok((KeyNode::Opaque, response));
    }
    let mut fields = Vec::with_capacity(args.len());
    for arg in args {
        let ty = arg.get("ty").ok_or_else(|| bad("argument without `ty`"))?;
        fields.push((
            str_of(arg, "name")?.to_owned(),
            closure.node(ty).shifted(base),
        ));
    }
    let wrapper = tables.intern_one(KeyStruct { fields });
    Ok((KeyNode::Struct(wrapper), response))
}

/// `@@paged` from `@@paged` or `@@paged(...)`: the leading sigil-and-word.
fn attribute_name(raw: &Value) -> &str {
    let raw = raw.as_str().unwrap_or_default();
    let end = raw
        .find(|c: char| !(c == '@' || c == '_' || c.is_ascii_alphanumeric()))
        .unwrap_or(raw.len());
    &raw[..end]
}

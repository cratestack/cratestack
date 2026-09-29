//! One canonical JSON node per IR type, written out field by field so that
//! what enters the digest is a decision, not a serde side effect. See the
//! parent module for what is dropped, sorted and kept in order.

use serde_json::{Value, json};

use super::attribute_norm::normalize_attribute_text;
use crate::schema::{
    Attribute, AuthBlock, ConfigBlock, Datasource, EnumDecl, Field, McpConfig, MixinDecl, Model,
    ModelMcpExposure, Procedure, ProcedureArg, ProcedureMcpExposure, Query, Schema, TypeArity,
    TypeDecl, TypeRef, View,
};

pub(super) fn canonical_schema(schema: &Schema) -> Value {
    json!({
        "datasource": schema.datasource.as_ref().map(datasource),
        "auth": schema.auth.as_ref().map(auth),
        "config_blocks": sorted(&schema.config_blocks, |b| &b.name, config_block),
        "mixins": sorted(&schema.mixins, |m| &m.name, mixin),
        "models": sorted(&schema.models, |m| &m.name, model),
        "types": sorted(&schema.types, |t| &t.name, type_decl),
        "enums": sorted(&schema.enums, |e| &e.name, enum_decl),
        "procedures": sorted(&schema.procedures, |p| &p.name, procedure),
        "views": sorted(&schema.views, |v| &v.name, view),
        "queries": sorted(&schema.queries, |q| &q.name, query),
        "transport": schema.transport.as_str(),
        "extensions": schema.declared_extensions.iter().map(|e| e.as_str()).collect::<Vec<_>>(),
        "mcp": schema.mcp.as_ref().map(mcp_config),
    })
}

/// Maps `items` in name order.
fn sorted<T>(items: &[T], name: impl Fn(&T) -> &String, node: impl Fn(&T) -> Value) -> Vec<Value> {
    let mut refs: Vec<&T> = items.iter().collect();
    refs.sort_by(|a, b| name(a).cmp(name(b)));
    refs.into_iter().map(node).collect()
}

fn datasource(d: &Datasource) -> Value {
    let entries: Vec<Value> = d.entries.iter().map(|e| json!([e.key, e.value])).collect();
    json!({ "name": d.name, "entries": entries })
}

fn auth(a: &AuthBlock) -> Value {
    json!({ "name": a.name, "fields": sorted(&a.fields, |f| &f.name, field) })
}

fn config_block(b: &ConfigBlock) -> Value {
    json!({ "name": b.name, "entries": b.entries })
}

fn mixin(m: &MixinDecl) -> Value {
    json!({ "name": m.name, "fields": sorted(&m.fields, |f| &f.name, field) })
}

fn type_decl(t: &TypeDecl) -> Value {
    json!({ "name": t.name, "fields": sorted(&t.fields, |f| &f.name, field) })
}

fn model(m: &Model) -> Value {
    json!({
        "name": m.name,
        "fields": sorted(&m.fields, |f| &f.name, field),
        "attributes": attributes(&m.attributes),
        "mcp": m.mcp.as_ref().map(model_mcp),
    })
}

fn enum_decl(e: &EnumDecl) -> Value {
    let variants: Vec<&String> = e.variants.iter().map(|v| &v.name).collect();
    json!({ "name": e.name, "variants": variants })
}

fn field(f: &Field) -> Value {
    json!({ "name": f.name, "ty": type_ref(&f.ty), "attributes": attributes(&f.attributes) })
}

fn type_ref(t: &TypeRef) -> Value {
    let arity = match t.arity {
        TypeArity::Required => "required",
        TypeArity::Optional => "optional",
        TypeArity::List => "list",
    };
    let generic_args: Vec<Value> = t.generic_args.iter().map(type_ref).collect();
    json!({
        "name": t.name,
        "arity": arity,
        "generic_args": generic_args,
        "int_args": t.int_args,
        "ident_args": t.ident_args,
    })
}

fn attributes(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .map(|a| normalize_attribute_text(&a.raw))
        .collect()
}

fn args(args: &[ProcedureArg]) -> Vec<Value> {
    args.iter()
        .map(|a| json!({ "name": a.name, "ty": type_ref(&a.ty) }))
        .collect()
}

fn procedure(p: &Procedure) -> Value {
    json!({
        "name": p.name,
        "kind": format!("{:?}", p.kind),
        "args": args(&p.args),
        "return_type": type_ref(&p.return_type),
        "attributes": attributes(&p.attributes),
        "mcp": p.mcp.as_ref().map(procedure_mcp),
    })
}

fn view(v: &View) -> Value {
    let sources: Vec<&String> = v.sources.iter().map(|s| &s.name).collect();
    json!({
        "name": v.name,
        "sources": sources,
        "fields": sorted(&v.fields, |f| &f.name, field),
        "attributes": attributes(&v.attributes),
    })
}

fn query(q: &Query) -> Value {
    json!({
        "name": q.name,
        "args": args(&q.args),
        "result_type": type_ref(&q.result_type),
        "attributes": attributes(&q.attributes),
    })
}

fn mcp_config(c: &McpConfig) -> Value {
    json!({
        "expose_tools": c.expose_tools.is_some(),
        "expose_resources": c.expose_resources.is_some(),
        "name": c.name.as_ref().map(|n| &n.value),
    })
}

fn model_mcp(m: &ModelMcpExposure) -> Value {
    json!({ "resource": m.resource, "max_page_size": m.max_page_size })
}

fn procedure_mcp(p: &ProcedureMcpExposure) -> Value {
    json!({
        "tool_name": p.tool_name,
        "tool_name_defaulted": p.tool_name_defaulted,
        "description": p.description,
    })
}

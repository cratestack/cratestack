//! Builders for the members shared by the declarations: fields, types,
//! attributes, procedures, views, queries and the MCP nodes. Every one
//! destructures its IR node exhaustively (see [`super::nodes`]).

use super::attribute_norm::normalize_attribute_text;
use super::canon::*;
use crate::schema::{
    Attribute, Field, McpConfig, McpName, ModelMcpExposure, Procedure, ProcedureArg, ProcedureKind,
    ProcedureMcpExposure, Query, TypeArity, TypeRef, View, ViewSource,
};

/// Maps `items` in name order.
pub(super) fn sorted<'a, T, N>(
    items: &'a [T],
    name: impl Fn(&T) -> &String,
    node: impl Fn(&'a T) -> N,
) -> Vec<N> {
    let mut refs: Vec<&T> = items.iter().collect();
    refs.sort_by(|a, b| name(a).cmp(name(b)));
    refs.into_iter().map(node).collect()
}

pub(super) fn field(f: &Field) -> CField<'_> {
    let Field {
        docs: _,
        name,
        name_span: _,
        ty,
        attributes: attrs,
        span: _,
    } = f;
    CField {
        attributes: attributes(attrs),
        name,
        ty: type_ref(ty),
    }
}

pub(super) fn type_ref(t: &TypeRef) -> CTypeRef<'_> {
    let TypeRef {
        name,
        name_span: _,
        arity,
        generic_args,
        int_args,
        ident_args,
    } = t;
    CTypeRef {
        arity: match arity {
            TypeArity::Required => "required",
            TypeArity::Optional => "optional",
            TypeArity::List => "list",
        },
        generic_args: generic_args.iter().map(type_ref).collect(),
        ident_args,
        int_args,
        name,
    }
}

pub(super) fn attributes(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .map(|a| {
            let Attribute { raw, span: _ } = a;
            normalize_attribute_text(raw)
        })
        .collect()
}

pub(super) fn args(args: &[ProcedureArg]) -> Vec<CArg<'_>> {
    args.iter()
        .map(|a| {
            let ProcedureArg {
                docs: _,
                name,
                name_span: _,
                ty,
                span: _,
            } = a;
            CArg {
                name,
                ty: type_ref(ty),
            }
        })
        .collect()
}

pub(super) fn procedure(p: &Procedure) -> CProcedure<'_> {
    let Procedure {
        docs: _,
        name,
        name_span: _,
        kind,
        args: procedure_args,
        return_type,
        attributes: attrs,
        span: _,
        mcp,
    } = p;
    CProcedure {
        args: args(procedure_args),
        attributes: attributes(attrs),
        kind: match kind {
            ProcedureKind::Query => "query",
            ProcedureKind::Mutation => "mutation",
        },
        mcp: mcp.as_ref().map(procedure_mcp),
        name,
        return_type: type_ref(return_type),
    }
}

pub(super) fn view(v: &View) -> CView<'_> {
    let View {
        docs: _,
        name,
        name_span: _,
        sources,
        fields,
        attributes: attrs,
        span: _,
    } = v;
    let sources = sources
        .iter()
        .map(|s| {
            let ViewSource { name, name_span: _ } = s;
            name.as_str()
        })
        .collect();
    CView {
        attributes: attributes(attrs),
        fields: sorted(fields, |f| &f.name, field),
        name,
        sources,
    }
}

pub(super) fn query(q: &Query) -> CQuery<'_> {
    let Query {
        docs: _,
        name,
        name_span: _,
        args: query_args,
        result_type,
        attributes: attrs,
        span: _,
    } = q;
    CQuery {
        args: args(query_args),
        attributes: attributes(attrs),
        name,
        result_type: type_ref(result_type),
    }
}

pub(super) fn mcp_config(c: &McpConfig) -> CMcpConfig<'_> {
    let McpConfig {
        docs: _,
        expose_tools,
        expose_resources,
        name,
        span: _,
    } = c;
    CMcpConfig {
        expose_resources: expose_resources.is_some(),
        expose_tools: expose_tools.is_some(),
        name: name.as_ref().map(|n| {
            let McpName { value, span: _ } = n;
            value.as_str()
        }),
    }
}

pub(super) fn model_mcp(m: &ModelMcpExposure) -> CModelMcp {
    let ModelMcpExposure {
        resource,
        max_page_size,
        span: _,
    } = m;
    CModelMcp {
        max_page_size: *max_page_size,
        resource: resource.clone(),
    }
}

pub(super) fn procedure_mcp(p: &ProcedureMcpExposure) -> CProcedureMcp<'_> {
    let ProcedureMcpExposure {
        tool_name,
        tool_name_defaulted,
        description,
        span: _,
    } = p;
    CProcedureMcp {
        description,
        tool_name,
        tool_name_defaulted: *tool_name_defaulted,
    }
}

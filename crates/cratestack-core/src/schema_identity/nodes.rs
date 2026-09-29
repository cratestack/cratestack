//! Builders for the top-level declarations, each destructuring its IR node
//! exhaustively so a new IR field is a compile error until it is encoded or
//! dropped on purpose. See the parent module for what is dropped, sorted and
//! kept in order.

use super::canon::*;
use super::members::{attributes, field, mcp_config, model_mcp, procedure, query, sorted, view};
use crate::schema::{
    AuthBlock, ConfigBlock, ConfigEntry, Datasource, EnumDecl, EnumVariant, MixinDecl, Model,
    Schema, TypeDecl,
};

/// The canonical bytes hashed under the domain tag (exposed so a test can
/// pin them exactly).
pub(super) fn canonical_bytes(schema: &Schema) -> Vec<u8> {
    serde_json::to_vec(&canonical_schema(schema)).expect("plain structs always serialize")
}

fn canonical_schema(schema: &Schema) -> CSchema<'_> {
    let Schema {
        datasource: datasource_decl,
        auth: auth_decl,
        config_blocks,
        mixins,
        models,
        types,
        enums,
        procedures,
        views,
        queries,
        transport,
        declared_extensions,
        mcp,
    } = schema;
    CSchema {
        auth: auth_decl.as_ref().map(auth),
        config_blocks: sorted(config_blocks, |b| &b.name, config_block),
        datasource: datasource_decl.as_ref().map(datasource),
        enums: sorted(enums, |e| &e.name, enum_decl),
        extensions: declared_extensions.iter().map(|e| e.as_str()).collect(),
        mcp: mcp.as_ref().map(mcp_config),
        mixins: sorted(mixins, |m| &m.name, mixin),
        models: sorted(models, |m| &m.name, model),
        procedures: sorted(procedures, |p| &p.name, procedure),
        queries: sorted(queries, |q| &q.name, query),
        transport: transport.as_str(),
        types: sorted(types, |t| &t.name, type_decl),
        views: sorted(views, |v| &v.name, view),
    }
}

fn datasource(d: &Datasource) -> CDatasource<'_> {
    let Datasource {
        docs: _,
        name,
        name_span: _,
        entries,
        span: _,
    } = d;
    let entries = entries
        .iter()
        .map(|e| {
            let ConfigEntry { key, value } = e;
            [key.as_str(), value.as_str()]
        })
        .collect();
    CDatasource { entries, name }
}

fn auth(a: &AuthBlock) -> CAuth<'_> {
    let AuthBlock {
        docs: _,
        name,
        name_span: _,
        fields,
        span: _,
    } = a;
    CAuth {
        fields: sorted(fields, |f| &f.name, field),
        name,
    }
}

fn config_block(b: &ConfigBlock) -> CConfigBlock<'_> {
    let ConfigBlock {
        docs: _,
        name,
        entries,
        span: _,
    } = b;
    CConfigBlock { entries, name }
}

fn mixin(m: &MixinDecl) -> CFields<'_> {
    let MixinDecl {
        docs: _,
        name,
        name_span: _,
        fields,
        span: _,
    } = m;
    CFields {
        fields: sorted(fields, |f| &f.name, field),
        name,
    }
}

fn type_decl(t: &TypeDecl) -> CFields<'_> {
    let TypeDecl {
        docs: _,
        name,
        name_span: _,
        fields,
        span: _,
    } = t;
    CFields {
        fields: sorted(fields, |f| &f.name, field),
        name,
    }
}

fn model(m: &Model) -> CModel<'_> {
    let Model {
        docs: _,
        name,
        name_span: _,
        fields,
        attributes: attrs,
        span: _,
        mcp,
    } = m;
    CModel {
        attributes: attributes(attrs),
        fields: sorted(fields, |f| &f.name, field),
        mcp: mcp.as_ref().map(model_mcp),
        name,
    }
}

fn enum_decl(e: &EnumDecl) -> CEnum<'_> {
    let EnumDecl {
        docs: _,
        name,
        name_span: _,
        variants,
        span: _,
    } = e;
    let variants = variants
        .iter()
        .map(|v| {
            let EnumVariant {
                docs: _,
                name,
                span: _,
            } = v;
            name.as_str()
        })
        .collect();
    CEnum { name, variants }
}

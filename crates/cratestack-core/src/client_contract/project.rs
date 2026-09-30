//! Wire projections: a declaration as a peer decodes it. `@server_only`
//! fields are gone and attributes on the drop list are filtered; every IR
//! node is destructured exhaustively, so a new IR field is a compile error
//! until it is encoded or dropped on purpose.

use std::collections::BTreeSet;

use super::attrs::{is_dropped, is_server_only};
use super::canon::{CClosure, CWireModel};
use crate::schema::{
    Attribute, EnumDecl, EnumVariant, Field, Model, Schema, TypeDecl, TypeRef, View,
    computed_params_type_name,
};
use crate::schema_identity::canon::{CEnum, CField, CFields};
use crate::schema_identity::members::type_ref;

pub(super) fn wire_attributes(attrs: &[Attribute]) -> Vec<String> {
    let kept: Vec<Attribute> = attrs
        .iter()
        .filter(|a| !is_dropped(&a.raw))
        .cloned()
        .collect();
    crate::schema_identity::members::attributes(&kept)
}

fn on_wire(field: &&Field) -> bool {
    !field.attributes.iter().any(|a| is_server_only(&a.raw))
}

fn wire_fields(fields: &[Field]) -> Vec<CField<'_>> {
    let mut kept: Vec<&Field> = fields.iter().filter(on_wire).collect();
    kept.sort_by(|a, b| a.name.cmp(&b.name));
    kept.into_iter().map(wire_field).collect()
}

fn wire_field(f: &Field) -> CField<'_> {
    let Field {
        docs: _,
        name,
        name_span: _,
        ty,
        attributes,
        span: _,
    } = f;
    CField {
        attributes: wire_attributes(attributes),
        name,
        ty: type_ref(ty),
    }
}

fn wire_model(m: &Model) -> CWireModel<'_> {
    let Model {
        docs: _,
        name,
        name_span: _,
        fields,
        attributes,
        span: _,
        mcp: _,
    } = m;
    CWireModel {
        attributes: wire_attributes(attributes),
        fields: wire_fields(fields),
        name,
    }
}

fn wire_view(v: &View) -> CWireModel<'_> {
    let View {
        docs: _,
        name,
        name_span: _,
        sources: _,
        fields,
        attributes,
        span: _,
    } = v;
    CWireModel {
        attributes: wire_attributes(attributes),
        fields: wire_fields(fields),
        name,
    }
}

fn wire_type(t: &TypeDecl) -> CFields<'_> {
    let TypeDecl {
        docs: _,
        name,
        name_span: _,
        fields,
        span: _,
    } = t;
    CFields {
        fields: wire_fields(fields),
        name,
    }
}

fn wire_enum(e: &EnumDecl) -> CEnum<'_> {
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

/// Collects every type name a type ref mentions, generic arguments included.
pub(super) fn type_names<'a>(ty: &'a TypeRef, out: &mut Vec<&'a str>) {
    out.push(&ty.name);
    ty.generic_args.iter().for_each(|arg| type_names(arg, out));
}

/// The names one declaration's wire fields point at, including the
/// `@computed(params: T?)` input type.
fn field_refs<'a>(fields: &'a [Field], out: &mut Vec<&'a str>) {
    for field in fields.iter().filter(on_wire) {
        type_names(&field.ty, out);
        out.extend(computed_params_type_name(field));
    }
}

/// The transitive closure of `roots` through field types, generic
/// arguments, relations and computed-params types, each declaration in
/// its wire projection and sorted by name.
pub(super) fn closure<'a>(schema: &'a Schema, roots: &[&'a str]) -> CClosure<'a> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut pending: Vec<&str> = roots.to_vec();
    while let Some(name) = pending.pop() {
        if !seen.insert(name) {
            continue;
        }
        if let Some(m) = schema.models.iter().find(|m| m.name == name) {
            field_refs(&m.fields, &mut pending);
        }
        if let Some(t) = schema.types.iter().find(|t| t.name == name) {
            field_refs(&t.fields, &mut pending);
        }
        if let Some(v) = schema.views.iter().find(|v| v.name == name) {
            field_refs(&v.fields, &mut pending);
        }
    }
    let reach = |name: &String| seen.contains(name.as_str());
    CClosure {
        enums: sorted_if(&schema.enums, |e| &e.name, reach, wire_enum),
        models: sorted_if(&schema.models, |m| &m.name, reach, wire_model),
        types: sorted_if(&schema.types, |t| &t.name, reach, wire_type),
        views: sorted_if(&schema.views, |v| &v.name, reach, wire_view),
    }
}

fn sorted_if<'a, T, N>(
    items: &'a [T],
    name: impl Fn(&T) -> &String,
    keep: impl Fn(&String) -> bool,
    node: impl Fn(&'a T) -> N,
) -> Vec<N> {
    let mut kept: Vec<&'a T> = items.iter().filter(|i| keep(name(i))).collect();
    kept.sort_by(|a, b| name(a).cmp(name(b)));
    kept.into_iter().map(node).collect()
}

//! The canonical node shapes. Fields are declared in lexicographic order:
//! `serde_json` writes struct fields in declaration order, whatever Cargo
//! features the consumer's graph unifies (a `Value` map flips under
//! `preserve_order`), so the bytes depend on this file alone.

use serde::Serialize;

#[derive(Serialize)]
pub(super) struct CSchema<'a> {
    pub(super) auth: Option<CAuth<'a>>,
    pub(super) config_blocks: Vec<CConfigBlock<'a>>,
    pub(super) datasource: Option<CDatasource<'a>>,
    pub(super) enums: Vec<CEnum<'a>>,
    pub(super) extensions: Vec<&'a str>,
    pub(super) mcp: Option<CMcpConfig<'a>>,
    pub(super) mixins: Vec<CFields<'a>>,
    pub(super) models: Vec<CModel<'a>>,
    pub(super) procedures: Vec<CProcedure<'a>>,
    pub(super) queries: Vec<CQuery<'a>>,
    /// Absent, not empty, for a schema without any: the identity of every
    /// schema predating subscription procedures stays what it was.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) subscription_procedures: Vec<CProcedure<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) topics: Vec<CTopic<'a>>,
    pub(super) transport: &'a str,
    pub(super) types: Vec<CFields<'a>>,
    pub(super) views: Vec<CView<'a>>,
}

/// A named node that is only a field list (mixin, type, auth block).
#[derive(Serialize)]
pub(crate) struct CFields<'a> {
    pub(crate) fields: Vec<CField<'a>>,
    pub(crate) name: &'a str,
}

#[derive(Serialize)]
pub(super) struct CAuth<'a> {
    pub(super) fields: Vec<CField<'a>>,
    pub(super) name: &'a str,
}

#[derive(Serialize)]
pub(super) struct CConfigBlock<'a> {
    pub(super) entries: &'a [String],
    pub(super) name: &'a str,
}

#[derive(Serialize)]
pub(super) struct CDatasource<'a> {
    pub(super) entries: Vec<[&'a str; 2]>,
    pub(super) name: &'a str,
}

#[derive(Serialize)]
pub(super) struct CModel<'a> {
    pub(super) attributes: Vec<String>,
    pub(super) fields: Vec<CField<'a>>,
    pub(super) mcp: Option<CModelMcp<'a>>,
    pub(super) name: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CEnum<'a> {
    pub(crate) name: &'a str,
    /// Each variant's ordinal, in variant order. Skipped when no variant has
    /// one, so a string enum's bytes, and every digest over them, are
    /// unchanged by the field's existence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ordinals: Option<Vec<Option<i64>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reserved: Option<&'a [i64]>,
    pub(crate) variants: Vec<&'a str>,
}

/// `CEnum::ordinals` for `variants`: `None` unless a variant is numbered.
pub(crate) fn enum_ordinals(variants: &[crate::schema::EnumVariant]) -> Option<Vec<Option<i64>>> {
    variants
        .iter()
        .any(|v| v.ordinal.is_some())
        .then(|| variants.iter().map(|v| v.ordinal).collect())
}

#[derive(Serialize)]
pub(crate) struct CField<'a> {
    pub(crate) attributes: Vec<String>,
    pub(crate) name: &'a str,
    pub(crate) ty: CTypeRef<'a>,
}

#[derive(Serialize)]
pub(crate) struct CTypeRef<'a> {
    pub(crate) arity: &'static str,
    pub(crate) generic_args: Vec<CTypeRef<'a>>,
    pub(crate) ident_args: &'a [String],
    pub(crate) int_args: &'a [u32],
    pub(crate) name: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CArg<'a> {
    pub(crate) name: &'a str,
    pub(crate) ty: CTypeRef<'a>,
}

#[derive(Serialize)]
pub(super) struct CProcedure<'a> {
    pub(super) args: Vec<CArg<'a>>,
    pub(super) attributes: Vec<String>,
    pub(super) kind: &'static str,
    pub(super) mcp: Option<CProcedureMcp<'a>>,
    pub(super) name: &'a str,
    pub(super) return_type: CTypeRef<'a>,
}

#[derive(Serialize)]
pub(super) struct CTopic<'a> {
    pub(super) key: CArg<'a>,
    pub(super) name: &'a str,
    pub(super) payload: CTypeRef<'a>,
}

#[derive(Serialize)]
pub(super) struct CView<'a> {
    pub(super) attributes: Vec<String>,
    pub(super) fields: Vec<CField<'a>>,
    pub(super) name: &'a str,
    pub(super) sources: Vec<&'a str>,
}

#[derive(Serialize)]
pub(super) struct CQuery<'a> {
    pub(super) args: Vec<CArg<'a>>,
    pub(super) attributes: Vec<String>,
    pub(super) name: &'a str,
    pub(super) result_type: CTypeRef<'a>,
}

#[derive(Serialize)]
pub(super) struct CMcpConfig<'a> {
    pub(super) expose_resources: bool,
    pub(super) expose_tools: bool,
    pub(super) name: Option<&'a str>,
}

#[derive(Serialize)]
pub(super) struct CModelMcp<'a> {
    pub(super) max_page_size: Option<u32>,
    pub(super) resource: &'a str,
}

#[derive(Serialize)]
pub(super) struct CProcedureMcp<'a> {
    pub(super) description: &'a Option<String>,
    pub(super) tool_name: &'a str,
    pub(super) tool_name_defaulted: bool,
}

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
    pub(super) transport: &'a str,
    pub(super) types: Vec<CFields<'a>>,
    pub(super) views: Vec<CView<'a>>,
}

/// A named node that is only a field list (mixin, type, auth block).
#[derive(Serialize)]
pub(super) struct CFields<'a> {
    pub(super) fields: Vec<CField<'a>>,
    pub(super) name: &'a str,
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
    pub(super) mcp: Option<CModelMcp>,
    pub(super) name: &'a str,
}

#[derive(Serialize)]
pub(super) struct CEnum<'a> {
    pub(super) name: &'a str,
    pub(super) variants: Vec<&'a str>,
}

#[derive(Serialize)]
pub(super) struct CField<'a> {
    pub(super) attributes: Vec<String>,
    pub(super) name: &'a str,
    pub(super) ty: CTypeRef<'a>,
}

#[derive(Serialize)]
pub(super) struct CTypeRef<'a> {
    pub(super) arity: &'static str,
    pub(super) generic_args: Vec<CTypeRef<'a>>,
    pub(super) ident_args: &'a [String],
    pub(super) int_args: &'a [u32],
    pub(super) name: &'a str,
}

#[derive(Serialize)]
pub(super) struct CArg<'a> {
    pub(super) name: &'a str,
    pub(super) ty: CTypeRef<'a>,
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
pub(super) struct CModelMcp {
    pub(super) max_page_size: Option<u32>,
    pub(super) resource: String,
}

#[derive(Serialize)]
pub(super) struct CProcedureMcp<'a> {
    pub(super) description: &'a Option<String>,
    pub(super) tool_name: &'a str,
    pub(super) tool_name_defaulted: bool,
}

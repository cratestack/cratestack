//! The canonical node shapes. Fields are declared in lexicographic order:
//! `serde_json` writes struct fields in declaration order, whatever Cargo
//! features the consumer's graph unifies (a `Value` map flips under
//! `preserve_order`), so the bytes depend on this file alone.

use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct CSchema<'a> {
    pub(crate) auth: Option<CAuth<'a>>,
    pub(crate) config_blocks: Vec<CConfigBlock<'a>>,
    pub(crate) datasource: Option<CDatasource<'a>>,
    pub(crate) enums: Vec<CEnum<'a>>,
    pub(crate) extensions: Vec<&'a str>,
    pub(crate) mcp: Option<CMcpConfig<'a>>,
    pub(crate) mixins: Vec<CFields<'a>>,
    pub(crate) models: Vec<CModel<'a>>,
    pub(crate) procedures: Vec<CProcedure<'a>>,
    pub(crate) queries: Vec<CQuery<'a>>,
    pub(crate) transport: &'a str,
    pub(crate) types: Vec<CFields<'a>>,
    pub(crate) views: Vec<CView<'a>>,
}

/// A named node that is only a field list (mixin, type, auth block).
#[derive(Serialize)]
pub(crate) struct CFields<'a> {
    pub(crate) fields: Vec<CField<'a>>,
    pub(crate) name: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CAuth<'a> {
    pub(crate) fields: Vec<CField<'a>>,
    pub(crate) name: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CConfigBlock<'a> {
    pub(crate) entries: &'a [String],
    pub(crate) name: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CDatasource<'a> {
    pub(crate) entries: Vec<[&'a str; 2]>,
    pub(crate) name: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CModel<'a> {
    pub(crate) attributes: Vec<String>,
    pub(crate) fields: Vec<CField<'a>>,
    pub(crate) mcp: Option<CModelMcp<'a>>,
    pub(crate) name: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CEnum<'a> {
    pub(crate) name: &'a str,
    pub(crate) variants: Vec<&'a str>,
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
pub(crate) struct CProcedure<'a> {
    pub(crate) args: Vec<CArg<'a>>,
    pub(crate) attributes: Vec<String>,
    pub(crate) kind: &'static str,
    pub(crate) mcp: Option<CProcedureMcp<'a>>,
    pub(crate) name: &'a str,
    pub(crate) return_type: CTypeRef<'a>,
}

#[derive(Serialize)]
pub(crate) struct CView<'a> {
    pub(crate) attributes: Vec<String>,
    pub(crate) fields: Vec<CField<'a>>,
    pub(crate) name: &'a str,
    pub(crate) sources: Vec<&'a str>,
}

#[derive(Serialize)]
pub(crate) struct CQuery<'a> {
    pub(crate) args: Vec<CArg<'a>>,
    pub(crate) attributes: Vec<String>,
    pub(crate) name: &'a str,
    pub(crate) result_type: CTypeRef<'a>,
}

#[derive(Serialize)]
pub(crate) struct CMcpConfig<'a> {
    pub(crate) expose_resources: bool,
    pub(crate) expose_tools: bool,
    pub(crate) name: Option<&'a str>,
}

#[derive(Serialize)]
pub(crate) struct CModelMcp<'a> {
    pub(crate) max_page_size: Option<u32>,
    pub(crate) resource: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CProcedureMcp<'a> {
    pub(crate) description: &'a Option<String>,
    pub(crate) tool_name: &'a str,
    pub(crate) tool_name_defaulted: bool,
}

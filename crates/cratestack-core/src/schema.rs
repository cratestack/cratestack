//! Schema IR — the parsed shape of a `.cstack` file. Every IR node
//! carries source-span back-pointers so consumers can map errors to
//! positions in the original text.

pub mod model;
pub mod procedure;
pub mod selection;

use serde::{Deserialize, Serialize};

pub use model::{Attribute, EnumDecl, EnumVariant, Field, MixinDecl, Model, TypeArity, TypeDecl, TypeRef};
pub use procedure::{Procedure, ProcedureArg, ProcedureKind};
pub use selection::SelectionQuery;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

/// Wire-shape the schema generates for. Picked once per schema (via
/// the top-level `transport rest|rpc` directive) so generated servers
/// and clients only carry one binding's worth of surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportStyle {
    #[default]
    Rest,
    Rpc,
}

impl TransportStyle {
    pub const fn as_str(&self) -> &'static str {
        match self {
            TransportStyle::Rest => "rest",
            TransportStyle::Rpc => "rpc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub datasource: Option<Datasource>,
    pub auth: Option<AuthBlock>,
    pub config_blocks: Vec<ConfigBlock>,
    pub mixins: Vec<MixinDecl>,
    pub models: Vec<Model>,
    pub types: Vec<TypeDecl>,
    pub enums: Vec<EnumDecl>,
    pub procedures: Vec<Procedure>,
    #[serde(default)]
    pub transport: TransportStyle,
}

impl Schema {
    pub fn summary(&self) -> OwnedSchemaSummary {
        OwnedSchemaSummary {
            mixins: self.mixins.iter().map(|mixin| mixin.name.clone()).collect(),
            models: self.models.iter().map(|model| model.name.clone()).collect(),
            types: self.types.iter().map(|ty| ty.name.clone()).collect(),
            enums: self
                .enums
                .iter()
                .map(|enum_decl| enum_decl.name.clone())
                .collect(),
            procedures: self
                .procedures
                .iter()
                .map(|procedure| procedure.name.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaSummary {
    pub mixins: &'static [&'static str],
    pub models: &'static [&'static str],
    pub types: &'static [&'static str],
    pub enums: &'static [&'static str],
    pub procedures: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedSchemaSummary {
    pub mixins: Vec<String>,
    pub models: Vec<String>,
    pub types: Vec<String>,
    pub enums: Vec<String>,
    pub procedures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Datasource {
    pub docs: Vec<String>,
    pub name: String,
    pub entries: Vec<ConfigEntry>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthBlock {
    pub docs: Vec<String>,
    pub name: String,
    pub fields: Vec<Field>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigBlock {
    pub docs: Vec<String>,
    pub name: String,
    pub entries: Vec<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

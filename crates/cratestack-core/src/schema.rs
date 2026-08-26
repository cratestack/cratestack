//! Schema IR — the parsed shape of a `.cstack` file. Every IR node
//! carries source-span back-pointers so consumers can map errors to
//! positions in the original text.

mod attribute_syntax;
pub mod composite_key;
pub mod composite_unique;
pub mod computed_attribute;
mod field_list;
pub mod index_attribute;
pub mod model;
pub mod procedure;
pub mod selection;
pub mod view;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub use composite_key::parse_composite_id_attribute;
pub use composite_unique::{ParsedCompositeUnique, parse_composite_unique_attribute};
pub use computed_attribute::{
    ComputedParamsArg, computed_params_type_name, is_computed_attribute, is_computed_field,
    parse_computed_params_arg,
};
pub use index_attribute::{ParsedIndexAttribute, parse_index_attribute};
pub use model::{
    Attribute, EnumDecl, EnumVariant, Field, MixinDecl, Model, TypeArity, TypeDecl, TypeRef,
};
pub use procedure::{Procedure, ProcedureArg, ProcedureKind};
pub use selection::SelectionQuery;
pub use view::{View, ViewSource};

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

/// An opt-in framework/database capability a schema announces via a
/// top-level `extension <name> { }` block (cratestack#153). Declaring an
/// extension only unlocks schema-visible *syntax* for that capability
/// (e.g. `@no_rate_limit`, the `Vector(n)` scalar type) — it never gates
/// codegen or runtime behavior by itself; that's a separate, same-named
/// Cargo feature per consuming crate (cratestack#161, out of scope here).
///
/// This is a closed list by design, not an arbitrary-extension mechanism
/// — see `docs/design/extensions.md` §7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionKind {
    RateLimit,
    Pgvector,
}

impl ExtensionKind {
    /// Every recognized extension name, in a stable order — used to build
    /// clear "expected one of: ..." error messages.
    pub const ALL: [ExtensionKind; 2] = [ExtensionKind::RateLimit, ExtensionKind::Pgvector];

    pub const fn as_str(&self) -> &'static str {
        match self {
            ExtensionKind::RateLimit => "rate_limit",
            ExtensionKind::Pgvector => "pgvector",
        }
    }

    /// Parses the bare name written after `extension` in `.cstack` source
    /// (e.g. `rate_limit` in `extension rate_limit { }`). `None` for any
    /// name outside the closed, framework-maintained list.
    pub fn parse_name(name: &str) -> Option<Self> {
        ExtensionKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == name)
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
    pub views: Vec<View>,
    #[serde(default)]
    pub transport: TransportStyle,
    /// Opt-in framework/database capabilities this schema declared via
    /// top-level `extension <name> { }` blocks (cratestack#153). Empty for
    /// every schema that declares none — no behavior change.
    #[serde(default)]
    pub declared_extensions: BTreeSet<ExtensionKind>,
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
            views: self.views.iter().map(|view| view.name.clone()).collect(),
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
    pub views: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedSchemaSummary {
    pub mixins: Vec<String>,
    pub models: Vec<String>,
    pub types: Vec<String>,
    pub enums: Vec<String>,
    pub procedures: Vec<String>,
    #[serde(default)]
    pub views: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_kind_as_str() {
        assert_eq!(ExtensionKind::RateLimit.as_str(), "rate_limit");
        assert_eq!(ExtensionKind::Pgvector.as_str(), "pgvector");
    }

    #[test]
    fn extension_kind_parse_name() {
        assert_eq!(
            ExtensionKind::parse_name("rate_limit"),
            Some(ExtensionKind::RateLimit)
        );
        assert_eq!(
            ExtensionKind::parse_name("pgvector"),
            Some(ExtensionKind::Pgvector)
        );
        assert_eq!(ExtensionKind::parse_name("unknown"), None);
    }

    #[test]
    fn extension_kind_all_constant() {
        assert_eq!(ExtensionKind::ALL.len(), 2);
        assert!(ExtensionKind::ALL.contains(&ExtensionKind::RateLimit));
        assert!(ExtensionKind::ALL.contains(&ExtensionKind::Pgvector));
    }

    #[test]
    fn extension_kind_serde_roundtrip() {
        for kind in ExtensionKind::ALL.iter() {
            let json = serde_json::to_string(kind).unwrap();
            let deserialized: ExtensionKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, deserialized);
        }
    }
}

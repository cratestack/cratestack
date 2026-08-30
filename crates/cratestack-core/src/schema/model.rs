//! Model / mixin / type / enum / field IR nodes parsed out of a
//! `.cstack` file. Every IR node carries [`SourceSpan`] back-pointers
//! so consumers (parser, LSP, generators) can map errors to source
//! positions.

use serde::{Deserialize, Serialize};

use super::SourceSpan;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub fields: Vec<Field>,
    pub attributes: Vec<Attribute>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixinDecl {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub fields: Vec<Field>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDecl {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub fields: Vec<Field>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDecl {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub variants: Vec<EnumVariant>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumVariant {
    pub docs: Vec<String>,
    pub name: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: TypeRef,
    pub attributes: Vec<Attribute>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRef {
    pub name: String,
    pub name_span: SourceSpan,
    pub arity: TypeArity,
    pub generic_args: Vec<TypeRef>,
    /// Compile-time integer literal arguments to a parametric scalar
    /// type, e.g. the `1536` in `Vector(1536)`. Empty for every type
    /// that isn't parametric — currently only `Vector(n)` populates
    /// this (see `docs/design/extensions.md` §6). `#[serde(default)]`
    /// keeps deserialization of migration snapshots written before
    /// this field existed working unchanged.
    #[serde(default)]
    pub int_args: Vec<u32>,
    /// Bare identifier arguments to a parametric scalar type, e.g. the
    /// `Polygon` in `Geography(Polygon, 4326)`. Empty for every type
    /// that doesn't take one — currently only the spatial scalars
    /// populate this (see `docs/design/extensions.md` §6b and
    /// cratestack#842). `#[serde(default)]` keeps deserialization of
    /// migration snapshots written before this field existed working
    /// unchanged, exactly as it does for `int_args`.
    #[serde(default)]
    pub ident_args: Vec<String>,
}

impl TypeRef {
    pub fn is_page(&self) -> bool {
        self.name == "Page"
    }

    pub fn page_item(&self) -> Option<&TypeRef> {
        if self.is_page() {
            self.generic_args.first()
        } else {
            None
        }
    }

    pub fn is_page_input(&self) -> bool {
        self.name == "PageInput"
    }

    pub fn is_find_many(&self) -> bool {
        self.name == "FindMany"
    }

    pub fn find_many_item(&self) -> Option<&TypeRef> {
        if self.is_find_many() {
            self.generic_args.first()
        } else {
            None
        }
    }

    pub fn is_vector(&self) -> bool {
        self.name == "Vector"
    }

    /// The `n` in `Vector(n)`, if this is a well-formed vector type
    /// reference (exactly one integer argument). Validation
    /// (`cratestack-parser`) is responsible for rejecting any other
    /// shape before this is relied upon by codegen.
    pub fn vector_dim(&self) -> Option<u32> {
        if self.is_vector() {
            self.int_args.first().copied()
        } else {
            None
        }
    }

    /// `true` for the PostGIS scalars `Geography` / `Geometry`
    /// (cratestack#842). Both are stored as EWKB and differ only in
    /// whether PostGIS computes on the spheroid (`geography`) or the
    /// plane (`geometry`).
    pub fn is_spatial(&self) -> bool {
        self.name == "Geography" || self.name == "Geometry"
    }

    /// `true` when this is the spheroidal `Geography` scalar, which is
    /// what the `ST_Covers`/`ST_DWithin`/`ST_Distance` builders in
    /// `cratestack-sql` assume as their operand.
    pub fn is_geography(&self) -> bool {
        self.name == "Geography"
    }

    /// The geometry subtype argument — the `Polygon` in
    /// `Geography(Polygon, 4326)` — if this is a spatial type
    /// reference carrying one. Validation (`cratestack-parser`) is
    /// responsible for rejecting any other shape before codegen relies
    /// on this, mirroring [`Self::vector_dim`].
    pub fn spatial_subtype(&self) -> Option<&str> {
        self.is_spatial()
            .then(|| self.ident_args.first().map(String::as_str))
            .flatten()
    }

    /// The SRID argument — the `4326` in `Geography(Polygon, 4326)`.
    /// `None` when the schema wrote the one-argument form
    /// (`Geography(Polygon)`), which defers to PostGIS's own default
    /// rather than inventing one here.
    pub fn spatial_srid(&self) -> Option<u32> {
        if self.is_spatial() {
            self.int_args.first().copied()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeArity {
    Required,
    Optional,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    pub raw: String,
    pub span: SourceSpan,
}

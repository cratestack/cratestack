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

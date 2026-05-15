//! Procedure / query-mutation IR nodes parsed out of a `.cstack` file.

use serde::{Deserialize, Serialize};

use super::SourceSpan;
use super::model::{Attribute, TypeRef};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Procedure {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub kind: ProcedureKind,
    pub args: Vec<ProcedureArg>,
    pub return_type: TypeRef,
    pub attributes: Vec<Attribute>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcedureKind {
    Query,
    Mutation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureArg {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: TypeRef,
    pub span: SourceSpan,
}

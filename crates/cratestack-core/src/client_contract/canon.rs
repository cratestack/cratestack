//! The canonical node shapes of an op contract. Fields are declared in
//! lexicographic order, so the bytes do not depend on `serde_json`'s
//! `preserve_order` feature (see `schema_identity::canon`). Type refs,
//! args, fields and enums reuse the schema identity's shapes.

use serde::Serialize;

use crate::schema_identity::canon::{CArg, CEnum, CField, CFields, CTypeRef};

#[derive(Serialize)]
pub(super) struct COpContract<'a> {
    pub(super) closure: CClosure<'a>,
    /// `subscribe` only: the event kinds `@@emit` declares.
    pub(super) events: Option<Vec<&'static str>>,
    pub(super) key: &'a str,
    pub(super) kind: &'static str,
    pub(super) model: Option<&'a str>,
    pub(super) procedure: Option<CWireProcedure<'a>>,
    pub(super) transport: &'static str,
    pub(super) verb: &'static str,
}

#[derive(Serialize)]
pub(super) struct CWireProcedure<'a> {
    pub(super) args: Vec<CArg<'a>>,
    pub(super) attributes: Vec<String>,
    pub(super) kind: &'static str,
    pub(super) name: &'a str,
    pub(super) return_type: CTypeRef<'a>,
}

/// Every declaration reachable from the op's roots, each by name.
#[derive(Serialize, Default)]
pub(super) struct CClosure<'a> {
    pub(super) enums: Vec<CEnum<'a>>,
    pub(super) models: Vec<CWireModel<'a>>,
    pub(super) types: Vec<CFields<'a>>,
    pub(super) views: Vec<CWireModel<'a>>,
}

/// A model or view in its wire projection.
#[derive(Serialize)]
pub(super) struct CWireModel<'a> {
    pub(super) attributes: Vec<String>,
    pub(super) fields: Vec<CField<'a>>,
    pub(super) name: &'a str,
}

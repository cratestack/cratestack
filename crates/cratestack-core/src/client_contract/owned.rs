//! An op contract read back from its canonical JSON: the side of the
//! compatibility check that comes from a lock file (or from
//! `cratestack contract print`). The shapes mirror `canon.rs` and refuse
//! unknown fields, so a contract this build cannot read in full is an
//! error (and so `Breaking`), never a partial comparison.

use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OpContract {
    pub(super) closure: Closure,
    pub(super) events: Option<Vec<String>>,
    pub(super) key: String,
    pub(super) kind: String,
    pub(super) model: Option<String>,
    pub(super) procedure: Option<Procedure>,
    pub(super) transport: String,
    pub(super) verb: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Procedure {
    pub(super) args: Vec<Arg>,
    pub(super) attributes: Vec<String>,
    pub(super) kind: String,
    pub(super) name: String,
    pub(super) return_type: TypeRef,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Arg {
    pub(super) name: String,
    pub(super) ty: TypeRef,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct TypeRef {
    pub(super) arity: String,
    pub(super) generic_args: Vec<TypeRef>,
    pub(super) ident_args: Vec<String>,
    pub(super) int_args: Vec<u32>,
    pub(super) name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct Closure {
    pub(super) enums: Vec<Enum>,
    pub(super) models: Vec<Decl>,
    pub(super) types: Vec<Decl>,
    pub(super) views: Vec<Decl>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Enum {
    pub(super) name: String,
    pub(super) variants: Vec<String>,
}

/// A model, view or type (types carry no attributes).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Decl {
    #[serde(default)]
    pub(super) attributes: Vec<String>,
    pub(super) fields: Vec<Field>,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Field {
    pub(super) attributes: Vec<String>,
    pub(super) name: String,
    pub(super) ty: TypeRef,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    Model,
    Type,
    View,
}

impl Kind {
    pub(super) const ALL: [Kind; 3] = [Kind::Model, Kind::Type, Kind::View];

    pub(super) fn word(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::Type => "type",
            Kind::View => "view",
        }
    }
}

impl TypeRef {
    /// Every declaration name this ref mentions, generic arguments included.
    pub(super) fn names<'a>(&'a self, out: &mut Vec<&'a str>) {
        out.push(&self.name);
        self.generic_args.iter().for_each(|arg| arg.names(out));
    }
}

impl Closure {
    pub(super) fn decls(&self, kind: Kind) -> &[Decl] {
        match kind {
            Kind::Model => &self.models,
            Kind::Type => &self.types,
            Kind::View => &self.views,
        }
    }

    /// Every declaration name reachable from `roots` through field types.
    pub(super) fn reach(&self, roots: &[&str]) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut pending: Vec<&str> = roots.to_vec();
        while let Some(name) = pending.pop() {
            if !seen.insert(name.to_owned()) {
                continue;
            }
            for kind in Kind::ALL {
                for decl in self.decls(kind).iter().filter(|d| d.name == name) {
                    for field in &decl.fields {
                        field.ty.names(&mut pending);
                    }
                }
            }
        }
        seen
    }
}

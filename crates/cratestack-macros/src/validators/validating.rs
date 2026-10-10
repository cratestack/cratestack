//! [`Validating`]: the `type`s and `model`s whose values need validating
//! when a client sends them, computed once per schema (ADR 0019 D5, PR A).
//!
//! A `type` or `model` is validating when one of the fields a client's
//! value fills carries a validator, or holds a validating `type` or
//! `model`: the macros give exactly these a `ValidateFields` impl
//! ([`super::types`]), and a procedure argument or a `@computed` params
//! `type` that names one is checked on its way in.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeDecl};

use crate::shared::{is_computed_field, is_server_only_field, model_name_set, scalar_model_fields};

use super::parse_field_validators;

/// The fields of `ty` a client's value fills: a `@computed` field is
/// resolved on the way out and never decoded from a client.
pub(super) fn stored_type_fields(ty: &TypeDecl) -> impl Iterator<Item = &Field> {
    ty.fields.iter().filter(|field| !is_computed_field(field))
}

/// The fields of `model` a client's value fills when the model is sent as a
/// value (a procedure argument, or a field of a `type`): the stored scalars.
/// A relation is a separate row, and a `@computed` field is resolved on the
/// way out. A `@server_only` field is `#[serde(skip)]` and always holds its
/// default, so a validator on it would judge a value the client never sent.
pub(super) fn stored_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> impl Iterator<Item = &'a Field> {
    scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_server_only_field(field))
}

fn holds_validator<'a>(mut fields: impl Iterator<Item = &'a Field>) -> bool {
    fields.any(|field| !parse_field_validators(field).is_empty())
}

#[derive(Debug, Default)]
pub(crate) struct Validating(BTreeSet<String>);

impl Validating {
    /// A schema with nothing to validate: the `query` blocks, whose
    /// arguments are bindable scalars, and tests.
    pub(crate) fn none() -> Self {
        Self::default()
    }

    /// The fixpoint over the schema: a declaration with a validator on a
    /// field a client fills, and any `type` with such a field of a
    /// validating `type` or `model`. A cycle (`type Node { children Node[] }`)
    /// terminates: a name enters the set once.
    pub(crate) fn of(types: &[TypeDecl], models: &[Model]) -> Self {
        let model_names = model_name_set(models);
        let mut names: BTreeSet<String> = models
            .iter()
            .filter(|model| holds_validator(stored_model_fields(model, &model_names)))
            .map(|model| model.name.clone())
            .chain(
                types
                    .iter()
                    .filter(|ty| holds_validator(stored_type_fields(ty)))
                    .map(|ty| ty.name.clone()),
            )
            .collect();
        // A model's stored fields are scalars, so only a `type` can enter
        // by holding another declaration.
        loop {
            let before = names.len();
            for ty in types {
                if !names.contains(&ty.name)
                    && stored_type_fields(ty).any(|field| names.contains(&field.ty.name))
                {
                    names.insert(ty.name.clone());
                }
            }
            if names.len() == before {
                return Self(names);
            }
        }
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.0.contains(name)
    }

    #[cfg(test)]
    pub(crate) fn names(&self) -> Vec<&str> {
        self.0.iter().map(String::as_str).collect()
    }
}

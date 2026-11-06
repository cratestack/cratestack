//! The fields of the embedded create input (cratestack#1234).
//!
//! The server create input leaves out every `@default` field: the
//! database or the auth context fills it. On the embedded runtime there is
//! no auth context, and only the app's own triggers or `DEFAULT`s can run
//! a database function, so:
//!
//! - an `auth().field` default stays in the input as declared, for the
//!   app to pass the current user's value;
//! - a `dbgenerated()` or registered-function default stays in as an
//!   `Option`: a set value is bound, and an unset one leaves the column to
//!   the database (`cratestack_rusqlite` refuses the create when a
//!   required column then comes back NULL);
//! - `now()`, `uuid()`, `cuid()` and literals stay out; the runtime binds
//!   them.

use std::collections::BTreeSet;

use cratestack_core::schema::default_functions::{DefaultValue, classify_default};
use cratestack_core::schema::default_markers::default_inner;
use cratestack_core::{Field, Model, TypeArity};

use crate::shared::{
    is_generated_on_create, is_readonly_field, is_server_only_field, is_version_field,
    scalar_model_fields,
};

/// A create-input field. `deferred` is set for a database-filled field,
/// whose `field` is the model field made optional.
pub(crate) struct CreateField {
    pub(crate) field: Field,
    pub(crate) deferred: bool,
}

impl CreateField {
    pub(crate) fn bound(field: &Field) -> Self {
        Self {
            field: field.clone(),
            deferred: false,
        }
    }

    fn deferred(field: &Field) -> Self {
        let mut field = field.clone();
        field.ty.arity = TypeArity::Optional;
        Self {
            field,
            deferred: true,
        }
    }

    /// The field with its value's own type, to bind a set value with.
    pub(crate) fn required(&self) -> Field {
        let mut field = self.field.clone();
        field.ty.arity = TypeArity::Required;
        field
    }
}

pub(crate) fn embedded_create_fields(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Vec<CreateField> {
    scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|f| !is_readonly_field(f) && !is_server_only_field(f) && !is_version_field(f))
        .filter_map(|field| {
            if !is_generated_on_create(field) {
                return Some(CreateField::bound(field));
            }
            match classify_default(default_inner(field)?) {
                DefaultValue::Auth => Some(CreateField::bound(field)),
                DefaultValue::Marker(_) | DefaultValue::Function(_)
                    if !matches!(field.ty.arity, TypeArity::List) =>
                {
                    Some(CreateField::deferred(field))
                }
                _ => None,
            }
        })
        .collect()
}

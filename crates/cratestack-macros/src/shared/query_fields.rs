//! The field set a request may filter or sort by.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model};

use super::{is_server_only_field, scalar_model_fields};

/// [`scalar_model_fields`] without `@server_only` fields: the keys a
/// request may name to filter (`?<field>__<op>=`, `where=`, `or=`, a
/// relation path, a `FindMany` `where`) or sort (`sort=`/`orderBy=`, a
/// `FindMany` `orderBy`) a list. REST and RPC share every one of these
/// parsers (`cratestack-axum/src/rpc/synthesize.rs`).
///
/// A value that is never sent must not be testable either: the list routes
/// took `scalar_model_fields`, so `?secret=HUNTER2` answered the row and
/// `?secret=WRONG` answered `[]`, and `?secret__startsWith=` rebuilt the
/// value one character at a time. Leaving the field out of every arm, sort
/// list and `FindMany` type makes it fall through to the same refusal as
/// a name the model does not declare, so the refusal cannot reveal that
/// the field exists either.
///
/// The typed server-side builders (`<model>::<field>()`, its relation
/// paths, `.order_by(...)`) keep `scalar_model_fields`: application code
/// may filter on a `@server_only` field, such as a login lookup by a
/// hashed token, and nothing there reads a request.
pub(crate) fn queryable_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_server_only_field(field))
        .collect()
}

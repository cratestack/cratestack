#![cfg(test)]

//! No-database coverage of `resolve_default_value`'s branches (via
//! `apply_create_defaults`, its `pub(crate)` entry point — see
//! `query/support/create.rs`). This is the runtime half of #431: a
//! `@default(auth().<path>)` column whose auth field is declared
//! required in the `auth` block must fail validation when that field
//! is absent from the caller's context, even when the model column
//! itself is nullable. CI without `CRATESTACK_TEST_DATABASE_URL` only
//! runs this file, not `cratestack-pg`'s PG-backed
//! `policy_db_auth_engine.rs`, so this is the only regression net for
//! this function on a plain `cargo test`.

use crate::query::apply_create_defaults;
use crate::{CreateDefault, CreateDefaultType, SqlValue};
use cratestack_core::{CoolContext, CoolError, Value};

fn required_string_default(column: &'static str, auth_field: &'static str) -> CreateDefault {
    CreateDefault {
        column,
        auth_field,
        ty: CreateDefaultType::String,
        nullable: true,
        auth_field_required: true,
    }
}

fn optional_string_default(
    column: &'static str,
    auth_field: &'static str,
    nullable: bool,
) -> CreateDefault {
    CreateDefault {
        column,
        auth_field,
        ty: CreateDefaultType::String,
        nullable,
        auth_field_required: false,
    }
}

/// A required auth field (per the `auth` block's own arity) missing
/// from the caller's context must fail validation, even though the
/// model column here is nullable — this is the exact tenant-isolation
/// bug #431 reports: `organizationId String? @default(auth().organization.id)`
/// silently resolving to NULL when `organization` is absent.
#[test]
fn missing_required_auth_field_is_a_validation_error() {
    let default = required_string_default("organization_id", "organization.id");
    let ctx = CoolContext::authenticated([("id".to_owned(), Value::String("usr_1".to_owned()))]);

    let err = apply_create_defaults(Vec::new(), &[default], &ctx)
        .expect_err("missing required auth field must fail validation");
    assert!(
        matches!(err, CoolError::Validation(_)),
        "expected Validation, got {err:?}"
    );
}

/// The mirror-image happy path: when the required auth field is
/// present, the default resolves normally and does not regress into
/// an error.
#[test]
fn present_required_auth_field_resolves_normally() {
    let default = required_string_default("organization_id", "organization.id");
    let ctx = CoolContext::authenticated([(
        "organization".to_owned(),
        Value::Map(std::collections::BTreeMap::from([(
            "id".to_owned(),
            Value::String("org_1".to_owned()),
        )])),
    )]);

    let values = apply_create_defaults(Vec::new(), &[default], &ctx)
        .expect("present required auth field should resolve");
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].column, "organization_id");
    assert_eq!(values[0].value, SqlValue::String("org_1".to_owned()));
}

/// A genuinely optional auth field (declared optional in the `auth`
/// block) that is missing, paired with a nullable model column, must
/// keep resolving to NULL — this is the "genuinely optional claim"
/// case #431 explicitly says must remain unaffected.
#[test]
fn missing_optional_auth_field_still_resolves_to_null() {
    let default = optional_string_default("nickname", "nickname", true);
    let ctx = CoolContext::authenticated([("id".to_owned(), Value::String("usr_1".to_owned()))]);

    let values = apply_create_defaults(Vec::new(), &[default], &ctx)
        .expect("missing optional auth field with nullable column should resolve to NULL");
    assert_eq!(values[0].value, SqlValue::NullString);
}

/// Pre-existing behavior, unchanged by #431: an optional auth field
/// that's missing but backed by a non-nullable model column must
/// still fail — an authenticated caller gets `Validation`, an
/// unauthenticated one gets `Forbidden`, both pinned here so the new
/// `auth_field_required` branch can't accidentally swallow them.
#[test]
fn missing_optional_auth_field_with_non_nullable_column_still_errors() {
    let default = optional_string_default("owner_id", "userId", false);

    let authenticated =
        CoolContext::authenticated([("id".to_owned(), Value::String("usr_1".to_owned()))]);
    let err = apply_create_defaults(Vec::new(), &[default], &authenticated)
        .expect_err("missing auth field for non-nullable column should fail validation");
    assert!(matches!(err, CoolError::Validation(_)));

    let anonymous = CoolContext::anonymous();
    let err = apply_create_defaults(Vec::new(), &[default], &anonymous)
        .expect_err("unauthenticated caller should be forbidden, not validation-failed");
    assert!(matches!(err, CoolError::Forbidden(_)));
}

/// Regression pin: an anonymous (unauthenticated) caller hitting a
/// *required*-auth-field default (e.g. `ScopedNote.ownerId
/// @default(auth().userId)`, where `userId` is required in the `auth`
/// block) must still get the pre-existing `Forbidden`, not the new
/// `Validation` branch. An anonymous context trivially has no auth
/// fields at all, so it would satisfy "missing" for *every* default —
/// checking `auth_field_required` before `!ctx.is_authenticated()`
/// would silently reclassify every anonymous-caller rejection on a
/// required-auth-field default from `Forbidden` to `Validation`. This
/// exact regression was caught by
/// `cratestack-pg`'s PG-backed `db_backed_auth_engine_supports_all_deny_and_auth_defaults`
/// (`anonymous_note_create`), not by any no-DB test — see #431's PR
/// for the initial (buggy) branch ordering this pins against.
#[test]
fn missing_required_auth_field_with_anonymous_caller_is_forbidden_not_validation() {
    let default = required_string_default("owner_id", "userId");
    let anonymous = CoolContext::anonymous();

    let err = apply_create_defaults(Vec::new(), &[default], &anonymous)
        .expect_err("anonymous caller must still be forbidden");
    assert!(
        matches!(err, CoolError::Forbidden(_)),
        "expected Forbidden, got {err:?}"
    );
}

/// Pre-existing behavior, unchanged by #431: an auth field present
/// with the wrong runtime type still fails as a type mismatch,
/// regardless of `auth_field_required`.
#[test]
fn present_auth_field_with_wrong_type_is_a_validation_error() {
    let default = required_string_default("organization_id", "organization");
    let ctx = CoolContext::authenticated([(
        "organization".to_owned(),
        Value::Int(1), // declared String, actual Int
    )]);

    let err = apply_create_defaults(Vec::new(), &[default], &ctx)
        .expect_err("type-mismatched auth field should fail validation");
    assert!(matches!(err, CoolError::Validation(_)));
}

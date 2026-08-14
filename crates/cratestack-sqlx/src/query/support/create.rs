//! Create-path support: auth-default filling + input-policy
//! evaluation (sync predicates + async EXISTS for relation references).

use cratestack_core::{CratestackContext, CratestackError, Value};
use cratestack_policy::{context_has_role, context_in_tenant};

use crate::{
    CreateDefault, CreateDefaultType, ReadPolicy, ReadPredicate, SqlColumnValue, SqlValue, sqlx,
};

use super::create_eval::evaluate_create_policy_expr;
use super::values::{
    auth_value_to_sql, find_column_value, sql_value_matches_literal, value_matches_auth_literal,
};

pub(crate) async fn evaluate_create_policies(
    pool: &sqlx::PgPool,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> Result<bool, CratestackError> {
    if allow_policies.is_empty() {
        return Ok(false);
    }

    for policy in deny_policies {
        if evaluate_create_policy_expr(pool, policy.expr, values, ctx).await? {
            return Ok(false);
        }
    }

    for policy in allow_policies {
        if evaluate_create_policy_expr(pool, policy.expr, values, ctx).await? {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(crate) fn apply_create_defaults(
    mut values: Vec<SqlColumnValue>,
    defaults: &[CreateDefault],
    ctx: &CratestackContext,
) -> Result<Vec<SqlColumnValue>, CratestackError> {
    for default in defaults {
        if find_column_value(&values, default.column).is_some() {
            continue;
        }
        let value = resolve_default_value(default, ctx)?;
        values.push(SqlColumnValue {
            column: default.column,
            value,
        });
    }
    Ok(values)
}

/// Resolve a default value from the auth context.
///
/// # Semantics
///
/// A required auth field (declared non-optional in the auth block) cannot be
/// silently absent, even if the model field is nullable. This enforces the
/// invariant that the auth block's declared shape is honored at runtime,
/// preventing tenant-isolation bugs where NULL values bypass policy predicates.
///
/// The semantics:
///
/// 1. **Auth field present, correct type**: Apply the value.
/// 2. **Auth field present, wrong type**: Error (type mismatch).
/// 3. **Auth field absent, context anonymous**: `Forbidden` unconditionally,
///    checked *before* `auth_field_required` — an unauthenticated caller
///    gets the pre-existing blanket policy-shaped rejection rather than a
///    `Validation` error that would leak which auth claim the schema
///    expects. (This ordering matters: an anonymous context trivially
///    fails `auth_field_required` too, since it has no fields at all, so
///    checking required-ness first here would silently turn every
///    anonymous-caller `Forbidden` into a `Validation` — this exact
///    regression was caught by `db_backed_auth_engine_supports_all_deny_and_auth_defaults`'s
///    pre-existing `anonymous_note_create` assertion on `ScopedNote`, whose
///    `ownerId @default(auth().userId)` references a *required* auth field.)
/// 4. **Auth field absent, auth field required, caller authenticated**:
///    Error unconditionally (regardless of model field nullability) — the
///    auth block declared this field as required, so an authenticated
///    context missing it is invalid.
/// 5. **Auth field absent, auth field optional, model field nullable**:
///    Return NULL (both are nullable, so missing is OK).
/// 6. **Auth field absent, model field non-nullable**: Error.
fn resolve_default_value(
    default: &CreateDefault,
    ctx: &CratestackContext,
) -> Result<SqlValue, CratestackError> {
    match ctx.auth_field(default.auth_field) {
        // Auth field is present — extract and validate type
        Some(Value::Bool(value)) => match default.ty {
            CreateDefaultType::Bool => Ok(SqlValue::Bool(*value)),
            _ => Err(CratestackError::Validation(format!(
                "auth field `{}` has incompatible type for create default on `{}`",
                default.auth_field, default.column
            ))),
        },
        Some(Value::Int(value)) => match default.ty {
            CreateDefaultType::Int => Ok(SqlValue::Int(*value)),
            _ => Err(CratestackError::Validation(format!(
                "auth field `{}` has incompatible type for create default on `{}`",
                default.auth_field, default.column
            ))),
        },
        Some(Value::String(value)) => match default.ty {
            CreateDefaultType::String => Ok(SqlValue::String(value.clone())),
            _ => Err(CratestackError::Validation(format!(
                "auth field `{}` has incompatible type for create default on `{}`",
                default.auth_field, default.column
            ))),
        },
        Some(_) => Err(CratestackError::Validation(format!(
            "auth field `{}` has incompatible type for create default on `{}`",
            default.auth_field, default.column
        ))),

        // Auth field is absent
        None if !ctx.is_authenticated() => {
            // Cannot apply defaults to unauthenticated contexts — checked
            // ahead of `auth_field_required` deliberately, see the
            // doc comment above.
            Err(CratestackError::Forbidden(
                "create policy denied this operation".to_owned(),
            ))
        }
        None if default.auth_field_required => {
            // Authenticated, but the required auth field is missing —
            // always an error, regardless of model field nullability.
            Err(CratestackError::Validation(format!(
                "missing required auth field `{}` for create default on `{}`",
                default.auth_field, default.column
            )))
        }
        None if default.nullable => {
            // Both model field and auth field are optional — NULL is OK
            match default.ty {
                CreateDefaultType::Bool => Ok(SqlValue::NullBool),
                CreateDefaultType::Int => Ok(SqlValue::NullInt),
                CreateDefaultType::String => Ok(SqlValue::NullString),
            }
        }
        None => {
            // Auth field is absent, model field is non-nullable
            Err(CratestackError::Validation(format!(
                "missing auth field `{}` required for create default on `{}`",
                default.auth_field, default.column
            )))
        }
    }
}

pub(crate) fn evaluate_input_predicate(
    predicate: ReadPredicate,
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> bool {
    match predicate {
        ReadPredicate::AuthNotNull => ctx.is_authenticated(),
        ReadPredicate::AuthIsNull => !ctx.is_authenticated(),
        ReadPredicate::AuthIsSystem => ctx.is_system(),
        ReadPredicate::HasRole { role } => context_has_role(ctx, role),
        ReadPredicate::InTenant { tenant_id } => context_in_tenant(ctx, tenant_id),
        ReadPredicate::AuthFieldEqLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| value_matches_auth_literal(candidate, value)),
        ReadPredicate::AuthFieldNeLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| !value_matches_auth_literal(candidate, value)),
        ReadPredicate::FieldIsTrue { column } => {
            find_column_value(values, column) == Some(&SqlValue::Bool(true))
        }
        ReadPredicate::FieldEqLiteral { column, value } => find_column_value(values, column)
            .is_some_and(|candidate| sql_value_matches_literal(candidate, value)),
        ReadPredicate::FieldNeLiteral { column, value } => find_column_value(values, column)
            .is_some_and(|candidate| !sql_value_matches_literal(candidate, value)),
        ReadPredicate::FieldEqAuth { column, auth_field } => match (
            find_column_value(values, column),
            auth_value_to_sql(ctx, auth_field),
        ) {
            (Some(candidate), Some(auth_value)) => candidate == &auth_value,
            _ => false,
        },
        ReadPredicate::FieldNeAuth { column, auth_field } => match (
            find_column_value(values, column),
            auth_value_to_sql(ctx, auth_field),
        ) {
            (Some(candidate), Some(auth_value)) => candidate != &auth_value,
            _ => false,
        },
        ReadPredicate::Relation { .. } => false,
    }
}

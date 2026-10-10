//! Create-path support: auth-default filling + input-policy
//! evaluation (sync predicates + async EXISTS for relation references).

use cratestack_core::{CratestackContext, CratestackError, Value};
use cratestack_policy::{context_has_role, context_in_tenant};

use crate::{
    CreateDefault, CreateDefaultType, ReadPolicy, ReadPredicate, SqlColumnValue, SqlValue,
};

use super::comparison::{Truth, claim_vs_literal, column_vs_claim, column_vs_literal};
use super::create_eval::evaluate_create_policy_expr;
use super::db::PolicyDb;
use super::values::{auth_value_to_sql, find_column_value};

pub(crate) async fn evaluate_create_policies(
    mut db: PolicyDb<'_>,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> Result<bool, CratestackError> {
    if allow_policies.is_empty() {
        return Ok(false);
    }

    // Three-valued, as SQL is on the other actions (see `super::comparison`):
    // a `@deny` fires on anything that is not `False`, `Unknown` included; an
    // `@allow` grants only on `True`.
    for policy in deny_policies {
        let truth = evaluate_create_policy_expr(db.reborrow(), policy.expr, values, ctx).await?;
        if !truth.is_false() {
            return Ok(false);
        }
    }

    for policy in allow_policies {
        let truth = evaluate_create_policy_expr(db.reborrow(), policy.expr, values, ctx).await?;
        if truth.is_true() {
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
            CreateDefaultType::BigInt => Ok(SqlValue::BigInt(*value)),
            _ => Err(CratestackError::Validation(format!(
                "auth field `{}` has incompatible type for create default on `{}`",
                default.auth_field, default.column
            ))),
        },
        Some(Value::String(value)) => match default.ty {
            CreateDefaultType::String => Ok(SqlValue::String(value.clone())),
            // A `BigInt` claim may arrive as its canonical string, the one
            // form the wire uses; anything else is refused, never coerced.
            CreateDefaultType::BigInt => value
                .parse::<cratestack_core::BigInt>()
                .map(|parsed| SqlValue::BigInt(parsed.get()))
                .map_err(|_| {
                    CratestackError::Validation(format!(
                        "auth field `{}` is not a canonical BigInt for create default on `{}`",
                        default.auth_field, default.column
                    ))
                }),
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
                CreateDefaultType::BigInt => Ok(SqlValue::NullBigInt),
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

/// A leaf as an `@allow` reads it: true only on a decided match. The create
/// path calls [`evaluate_input_truth`], which a `@deny` also needs.
#[cfg(test)]
pub(crate) fn evaluate_input_predicate(
    predicate: ReadPredicate,
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> bool {
    evaluate_input_truth(predicate, values, ctx).is_true()
}

/// One leaf of a create policy, against the prospective input and the caller.
/// An absent operand makes it `False`, a pair that cannot be compared makes it
/// `Unknown`; see [`super::comparison`].
pub(crate) fn evaluate_input_truth(
    predicate: ReadPredicate,
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> Truth {
    let input = |column: &str| find_column_value(values, column);
    match predicate {
        ReadPredicate::AuthNotNull => ctx.is_authenticated().into(),
        ReadPredicate::AuthIsNull => (!ctx.is_authenticated()).into(),
        ReadPredicate::AuthIsSystem => ctx.is_system().into(),
        ReadPredicate::HasRole { role } => context_has_role(ctx, role).into(),
        ReadPredicate::InTenant { tenant_id } => context_in_tenant(ctx, tenant_id).into(),
        ReadPredicate::AuthFieldEqLiteral { auth_field, value } => {
            ctx.auth_field(auth_field).map_or(Truth::False, |claim| {
                claim_vs_literal(claim, value).for_eq()
            })
        }
        ReadPredicate::AuthFieldNeLiteral { auth_field, value } => {
            ctx.auth_field(auth_field).map_or(Truth::False, |claim| {
                claim_vs_literal(claim, value).for_ne()
            })
        }
        ReadPredicate::FieldIsTrue { column } => {
            (input(column) == Some(&SqlValue::Bool(true))).into()
        }
        ReadPredicate::FieldEqLiteral { column, value } => {
            input(column).map_or(Truth::False, |c| column_vs_literal(c, value).for_eq())
        }
        ReadPredicate::FieldNeLiteral { column, value } => {
            input(column).map_or(Truth::False, |c| column_vs_literal(c, value).for_ne())
        }
        // `in` is the `or` of its `==`, `not in` the `and` of its `!=`, as in
        // SQL. A column absent from the input is `False` for both: unstated,
        // it can be asserted neither inside a set nor outside it.
        ReadPredicate::FieldInLiterals {
            column,
            values: literals,
        } => input(column).map_or(Truth::False, |c| {
            literals.iter().fold(Truth::False, |truth, literal| {
                truth.or(column_vs_literal(c, *literal).for_eq())
            })
        }),
        ReadPredicate::FieldNotInLiterals {
            column,
            values: literals,
        } => input(column).map_or(Truth::False, |c| {
            literals.iter().fold(Truth::True, |truth, literal| {
                truth.and(column_vs_literal(c, *literal).for_ne())
            })
        }),
        ReadPredicate::FieldEqAuth { column, auth_field } => {
            match (input(column), auth_value_to_sql(ctx, auth_field)) {
                (Some(candidate), Some(claim)) => column_vs_claim(candidate, &claim).for_eq(),
                _ => Truth::False,
            }
        }
        ReadPredicate::FieldNeAuth { column, auth_field } => {
            match (input(column), auth_value_to_sql(ctx, auth_field)) {
                (Some(candidate), Some(claim)) => column_vs_claim(candidate, &claim).for_ne(),
                _ => Truth::False,
            }
        }
        ReadPredicate::Relation { .. } => Truth::False,
    }
}

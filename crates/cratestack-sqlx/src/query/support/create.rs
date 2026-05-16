//! Create-path support: auth-default filling + input-policy
//! evaluation (sync predicates + async EXISTS for relation references).

use cratestack_core::{CoolContext, CoolError, Value};
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
    ctx: &CoolContext,
) -> Result<bool, CoolError> {
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
    ctx: &CoolContext,
) -> Result<Vec<SqlColumnValue>, CoolError> {
    for default in defaults {
        if find_column_value(&values, default.column).is_some() {
            continue;
        }
        let value = resolve_default_value(default, ctx)?;
        values.push(SqlColumnValue { column: default.column, value });
    }
    Ok(values)
}

fn resolve_default_value(default: &CreateDefault, ctx: &CoolContext) -> Result<SqlValue, CoolError> {
    match (ctx.auth_field(default.auth_field), default.ty, default.nullable) {
        (Some(Value::Bool(value)), CreateDefaultType::Bool, _) => Ok(SqlValue::Bool(*value)),
        (Some(Value::Int(value)), CreateDefaultType::Int, _) => Ok(SqlValue::Int(*value)),
        (Some(Value::String(value)), CreateDefaultType::String, _) => Ok(SqlValue::String(value.clone())),
        (None, CreateDefaultType::Bool, true) => Ok(SqlValue::NullBool),
        (None, CreateDefaultType::Int, true) => Ok(SqlValue::NullInt),
        (None, CreateDefaultType::String, true) => Ok(SqlValue::NullString),
        (None, _, false) if !ctx.is_authenticated() => Err(CoolError::Forbidden(
            "create policy denied this operation".to_owned(),
        )),
        (None, _, false) => Err(CoolError::Validation(format!(
            "missing auth field `{}` required for create default on `{}`",
            default.auth_field, default.column
        ))),
        (Some(_), _, _) => Err(CoolError::Validation(format!(
            "auth field `{}` has incompatible type for create default on `{}`",
            default.auth_field, default.column
        ))),
    }
}

pub(super) fn evaluate_input_predicate(
    predicate: ReadPredicate,
    values: &[SqlColumnValue],
    ctx: &CoolContext,
) -> bool {
    match predicate {
        ReadPredicate::AuthNotNull => ctx.is_authenticated(),
        ReadPredicate::AuthIsNull => !ctx.is_authenticated(),
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


use cratestack_core::{CratestackContext, Value};

use super::{missing_policy_columns, record_readable};
use crate::{
    PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, RelationQuantifier, SqlColumnValue,
    SqlValue,
};

const OWNER_IS_CALLER: ReadPolicy = ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
        column: "owner_id",
        auth_field: "id",
    }),
};
const ARCHIVED: ReadPolicy = ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::FieldIsTrue { column: "archived" }),
};
const SIGNED_IN: ReadPolicy = ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
};

fn record(owner: i64, archived: bool) -> Vec<SqlColumnValue> {
    vec![
        SqlColumnValue {
            column: "owner_id",
            value: SqlValue::Int(owner),
        },
        SqlColumnValue {
            column: "archived",
            value: SqlValue::Bool(archived),
        },
    ]
}

fn caller(id: i64) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(id))])
}

async fn readable(
    allow: &[ReadPolicy],
    deny: &[ReadPolicy],
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> bool {
    // No relation predicate, so the pool is never used.
    let pool = crate::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/none")
        .expect("a lazy pool does not connect");
    record_readable(&pool, allow, deny, values, ctx)
        .await
        .expect("no relation predicate, so no query")
}

#[tokio::test]
async fn a_record_is_readable_only_as_its_policy_says() {
    let allow = [OWNER_IS_CALLER];
    assert!(readable(&allow, &[], &record(7, false), &caller(7)).await);
    assert!(!readable(&allow, &[], &record(7, false), &caller(8)).await);
    assert!(
        !readable(
            &allow,
            &[],
            &record(7, false),
            &CratestackContext::anonymous()
        )
        .await
    );
}

#[tokio::test]
async fn a_deny_wins_and_no_allow_denies() {
    let deny = [ARCHIVED];
    assert!(!readable(&[SIGNED_IN], &deny, &record(7, true), &caller(7)).await);
    assert!(readable(&[SIGNED_IN], &deny, &record(7, false), &caller(7)).await);
    assert!(!readable(&[], &[], &record(7, false), &caller(7)).await);
}

#[test]
fn missing_columns_name_what_the_record_lacks() {
    assert!(
        missing_policy_columns(&[OWNER_IS_CALLER], &[ARCHIVED], &["owner_id", "archived"])
            .is_empty()
    );
    assert_eq!(
        missing_policy_columns(&[OWNER_IS_CALLER, SIGNED_IN], &[ARCHIVED], &["owner_id"]),
        ["archived"],
    );
}

#[test]
fn missing_columns_look_inside_combinators_but_not_across_a_relation() {
    static NESTED: [PolicyExpr; 2] = [
        PolicyExpr::Predicate(ReadPredicate::FieldInLiterals {
            column: "status",
            values: &[PolicyLiteral::String("open")],
        }),
        PolicyExpr::Predicate(ReadPredicate::FieldNeAuth {
            column: "owner_id",
            auth_field: "id",
        }),
    ];
    static RELATED: PolicyExpr = PolicyExpr::Predicate(ReadPredicate::FieldIsTrue {
        column: "only_on_the_related_table",
    });
    let allow = [
        ReadPolicy {
            expr: PolicyExpr::Or(&NESTED),
        },
        ReadPolicy {
            expr: PolicyExpr::Predicate(ReadPredicate::Relation {
                quantifier: RelationQuantifier::ToOne,
                parent_table: "posts",
                parent_column: "team_id",
                related_table: "teams",
                related_column: "id",
                expr: &RELATED,
            }),
        },
    ];
    assert_eq!(
        missing_policy_columns(&allow, &[], &[]),
        ["owner_id", "status", "team_id"]
    );
}

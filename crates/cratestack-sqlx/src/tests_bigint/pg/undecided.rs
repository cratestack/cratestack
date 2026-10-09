//! `auth().tenant <op> 7` with a claim that cannot be compared with an
//! integer, against Postgres, through the same functions every operation
//! calls. It is decided in-process and emitted as a constant; as `FALSE` it
//! made a `@deny` silent (`NOT (FALSE)` is true) on read, update and delete.
//! Now a `@deny` refuses the row and an `@allow` grants nothing, while an
//! integer claim still decides both ways.

use cratestack_core::{BigInt, CratestackContext, CratestackError, Value};

use super::fixture::{ALL, TABLE, all_except, claim, descriptor, leak, reseed};
use super::read::visible;
use crate::query::{authorize_record_action, push_action_policy_query};
use crate::sqlx::{self, PgPool};
use crate::tests_bigint::undecided_render::{eq_seven, ne_seven};
use crate::{PolicyExpr, ReadPolicy, ReadPredicate, RelationQuantifier, SqlxRuntime};

const CHILDREN: &str = "bigint_policy_children";

struct Case {
    label: &'static str,
    allow: &'static [ReadPolicy],
    deny: &'static [ReadPolicy],
    ctx: CratestackContext,
    /// Does the policy let a row through?
    admits: bool,
}

fn cases() -> Vec<Case> {
    let any = leak(ReadPredicate::AuthNotNull);
    let (eq, ne) = (leak(eq_seven()), leak(ne_seven()));
    let string = || claim(Value::String("7".to_owned()));
    let int = |value| claim(Value::Int(value));
    let mut cases = Vec::new();
    // `@deny`, with a blanket `@allow` beside it.
    for (label, deny, ctx, admits) in [
        ("deny == 7, claim \"7\"", eq, string(), false),
        ("deny != 7, claim \"7\"", ne, string(), false),
        ("deny == 7, claim true", eq, claim(Value::Bool(true)), false),
        ("deny != 7, claim null", ne, claim(Value::Null), false),
        ("deny == 7, claim 7", eq, int(7), false),
        ("deny == 7, claim 8", eq, int(8), true),
        ("deny != 7, claim 7", ne, int(7), true),
        ("deny != 7, claim 8", ne, int(8), false),
    ] {
        cases.push(Case {
            label,
            allow: any,
            deny,
            ctx,
            admits,
        });
    }
    // `@allow` alone.
    for (label, allow, ctx, admits) in [
        ("allow == 7, claim \"7\"", eq, string(), false),
        ("allow != 7, claim \"7\"", ne, string(), false),
        ("allow == 7, claim 7", eq, int(7), true),
        ("allow == 7, claim 8", eq, int(8), false),
        ("allow != 7, claim 7", ne, int(7), false),
        ("allow != 7, claim 8", ne, int(8), true),
    ] {
        cases.push(Case {
            label,
            allow,
            deny: &[],
            ctx,
            admits,
        });
    }
    cases
}

/// Read, the update and delete preflight, and the policy spliced into `UPDATE`
/// and `DELETE`: each admits every row or none.
pub(super) async fn an_undecidable_claim_denies_on_read_update_and_delete(pool: &PgPool) {
    let runtime = SqlxRuntime::new(pool.clone());
    for case in cases() {
        let Case {
            label,
            allow,
            deny,
            ref ctx,
            admits,
        } = case;
        reseed(pool).await;
        let seen = visible(pool, descriptor(allow, deny), ctx).await.unwrap();
        let expected = if admits { all_except(&[]) } else { vec![] };
        assert_eq!(seen, expected, "read: {label}");

        for action in ["update", "delete"] {
            let target = descriptor(allow, deny);
            let outcome =
                authorize_record_action(&runtime, target, BigInt::new(7), allow, deny, ctx, action)
                    .await;
            match (admits, &outcome) {
                (true, Ok(())) => {}
                (false, Err(CratestackError::Forbidden(_))) => {}
                _ => panic!("{action} preflight: {label}: {outcome:?}"),
            }
        }

        let rows = if admits { ALL.len() as u64 } else { 0 };
        let mut update = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "UPDATE bigint_policy_rows SET touched = TRUE WHERE ",
        );
        push_action_policy_query(&mut update, allow, deny, ctx);
        let touched = update.build().execute(pool).await.unwrap().rows_affected();
        assert_eq!(touched, rows, "UPDATE: {label}");

        let mut delete =
            sqlx::QueryBuilder::<sqlx::Postgres>::new("DELETE FROM bigint_policy_rows WHERE ");
        push_action_policy_query(&mut delete, allow, deny, ctx);
        let removed = delete.build().execute(pool).await.unwrap().rows_affected();
        assert_eq!(removed, rows, "DELETE: {label}");
    }
}

async fn reseed_children(pool: &PgPool) {
    reseed(pool).await;
    sqlx::query("DROP TABLE IF EXISTS bigint_policy_children")
        .execute(pool)
        .await
        .expect("drop");
    sqlx::query("CREATE TABLE bigint_policy_children (parent_id BIGINT NOT NULL)")
        .execute(pool)
        .await
        .expect("create");
    // Rows 0 and 7 have a child; the others have none.
    sqlx::query("INSERT INTO bigint_policy_children VALUES (0), (7)")
        .execute(pool)
        .await
        .expect("insert");
}

fn over_children(quantifier: RelationQuantifier) -> &'static [ReadPolicy] {
    let inner: &'static PolicyExpr = Box::leak(Box::new(PolicyExpr::Predicate(eq_seven())));
    leak(ReadPredicate::Relation {
        quantifier,
        parent_table: TABLE,
        parent_column: "id",
        related_table: CHILDREN,
        related_column: "parent_id",
        expr: inner,
    })
}

/// Inside a relation quantifier the constant stays `FALSE`. As NULL, `every`
/// (`NOT EXISTS (.. AND NOT (expr))`) would read "no counterexample" and grant
/// every parent that has children, for a claim that cannot be compared.
pub(super) async fn a_relation_quantifier_never_grants_on_an_undecidable_claim(pool: &PgPool) {
    reseed_children(pool).await;
    let childless = all_except(&[0, 7]);
    let with_children = vec![0, 7];
    let string = claim(Value::String("7".to_owned()));
    let rows = |quantifier, ctx| async move {
        visible(pool, descriptor(over_children(quantifier), &[]), &ctx)
            .await
            .unwrap()
    };

    // `every`: vacuously true for a parent with no children, and for the rest
    // only if the expression holds for each child.
    assert_eq!(rows(RelationQuantifier::Every, string).await, childless);
    assert_eq!(
        rows(RelationQuantifier::Every, claim(Value::Int(7))).await,
        all_except(&[])
    );
    assert_eq!(
        rows(RelationQuantifier::Every, claim(Value::Int(8))).await,
        childless
    );
    // `some`, `none`: EXISTS and NOT EXISTS are decided, so FALSE and NULL
    // agree; pinned so that stays true.
    let string = || claim(Value::String("7".to_owned()));
    assert_eq!(
        rows(RelationQuantifier::Some, string()).await,
        Vec::<i64>::new()
    );
    assert_eq!(
        rows(RelationQuantifier::Some, claim(Value::Int(7))).await,
        with_children
    );
    assert_eq!(
        rows(RelationQuantifier::None, string()).await,
        all_except(&[])
    );
    assert_eq!(
        rows(RelationQuantifier::None, claim(Value::Int(7))).await,
        childless
    );
    sqlx::query("DROP TABLE bigint_policy_children")
        .execute(pool)
        .await
        .expect("drop");
}

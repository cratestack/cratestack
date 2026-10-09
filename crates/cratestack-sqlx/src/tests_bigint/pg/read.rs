//! Reads: `BigInt` values and keys round-trip, and a negated policy hides the
//! matching row (and only it).

use cratestack_core::{BigInt, CratestackContext, Value};

use super::fixture::{ALL, all_except, claim, descriptor, leak, reseed};
use crate::query::{ReadPolicyKind, push_scoped_conditions};
use crate::sqlx::{self, PgPool};
use crate::{ModelDescriptor, PolicyLiteral, ReadPredicate};

static SET: [PolicyLiteral; 2] = [PolicyLiteral::Int(7), PolicyLiteral::Int(i64::MAX)];

pub(super) async fn visible(
    pool: &PgPool,
    descriptor: &'static ModelDescriptor<(), BigInt>,
    ctx: &CratestackContext,
) -> Result<Vec<i64>, sqlx::Error> {
    let mut query =
        sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT amount FROM bigint_policy_rows");
    push_scoped_conditions(
        &mut query,
        descriptor,
        &[],
        None::<(&'static str, BigInt)>,
        ctx,
        ReadPolicyKind::List,
    );
    query.push(" ORDER BY amount");
    query.build_query_scalar::<i64>().fetch_all(pool).await
}

pub(super) async fn round_trips_and_binds_a_bigint_primary_key(pool: &PgPool) {
    reseed(pool).await;
    let rows: Vec<(BigInt, BigInt, Option<BigInt>)> =
        sqlx::query_as("SELECT id, amount, owner FROM bigint_policy_rows ORDER BY id")
            .fetch_all(pool)
            .await
            .expect("BigInt decodes from INT8");
    let ids: Vec<i64> = rows.iter().map(|row| row.0.get()).collect();
    assert_eq!(ids, all_except(&[]), "stored and read back exactly");
    assert!(rows.iter().all(|row| row.2.is_none()), "NullBigInt is NULL");

    let allow = leak(ReadPredicate::AuthNotNull);
    let ctx = CratestackContext::authenticated([]);
    for key in ALL {
        let mut query =
            sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT amount FROM bigint_policy_rows");
        push_scoped_conditions(
            &mut query,
            descriptor(allow, &[]),
            &[],
            Some(("id", BigInt::new(key))),
            &ctx,
            ReadPolicyKind::List,
        );
        let found = query.build_query_scalar::<i64>().fetch_all(pool).await;
        assert_eq!(found.expect("PK lookup"), vec![key], "BigInt @id lookup");
    }
}

pub(super) async fn negated_policies_deny_on_read(pool: &PgPool) {
    reseed(pool).await;
    let anon = CratestackContext::anonymous();
    for value in ALL {
        let ne = leak(ReadPredicate::FieldNeLiteral {
            column: "amount",
            value: PolicyLiteral::Int(value),
        });
        let seen = visible(pool, descriptor(ne, &[]), &anon).await.unwrap();
        assert_eq!(seen, all_except(&[value]), "`amount != {value}`");

        let eq = leak(ReadPredicate::FieldEqLiteral {
            column: "amount",
            value: PolicyLiteral::Int(value),
        });
        let seen = visible(pool, descriptor(eq, &[]), &anon).await.unwrap();
        assert_eq!(seen, vec![value], "`amount == {value}`");

        let ne_auth = leak(ReadPredicate::FieldNeAuth {
            column: "amount",
            auth_field: "tenant",
        });
        let seen = visible(pool, descriptor(ne_auth, &[]), &claim(Value::Int(value)))
            .await
            .unwrap();
        assert_eq!(seen, all_except(&[value]), "`amount != auth().tenant`");

        let allow = leak(ReadPredicate::AuthNotNull);
        let deny = leak(ReadPredicate::FieldEqLiteral {
            column: "amount",
            value: PolicyLiteral::Int(value),
        });
        let seen = visible(pool, descriptor(allow, deny), &claim(Value::Int(0)))
            .await
            .unwrap();
        assert_eq!(seen, all_except(&[value]), "`@@deny amount == {value}`");
    }
    let not_in = leak(ReadPredicate::FieldNotInLiterals {
        column: "amount",
        values: &SET,
    });
    let seen = visible(pool, descriptor(not_in, &[]), &anon).await.unwrap();
    assert_eq!(seen, all_except(&[7, i64::MAX]), "`amount not in [7, MAX]`");
}

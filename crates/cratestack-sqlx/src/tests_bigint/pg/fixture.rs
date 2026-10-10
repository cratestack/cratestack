//! One table, one descriptor shape, and the rows every scenario starts from.

use cratestack_core::{BigInt, CratestackContext, Value};

use crate::query::push_bind_value;
use crate::sqlx::{self, PgPool};
use crate::{ModelColumn, ModelDescriptor, PolicyExpr, ReadPolicy, ReadPredicate, SqlValue};

pub(super) const TABLE: &str = "bigint_policy_rows";

/// Each row's `id` and `amount` are the same value.
pub(super) const ALL: [i64; 6] = [i64::MIN, -1, 0, 7, 9_007_199_254_740_993, i64::MAX];

static COLUMNS: [ModelColumn; 4] = [
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "amount",
        sql_name: "amount",
    },
    ModelColumn {
        rust_name: "owner",
        sql_name: "owner",
    },
    ModelColumn {
        rust_name: "touched",
        sql_name: "touched",
    },
];

pub(super) fn leak(predicate: ReadPredicate) -> &'static [ReadPolicy] {
    Box::leak(Box::new([ReadPolicy {
        expr: PolicyExpr::Predicate(predicate),
    }]))
}

pub(super) fn claim(value: Value) -> CratestackContext {
    CratestackContext::authenticated([("tenant".to_owned(), value)])
}

/// A descriptor whose read policies are the given ones, keyed by a `BigInt`.
pub(super) fn descriptor(
    read_allow: &'static [ReadPolicy],
    read_deny: &'static [ReadPolicy],
) -> &'static ModelDescriptor<(), BigInt> {
    Box::leak(Box::new(ModelDescriptor::<(), BigInt>::new(
        "BigIntPolicyRow",
        TABLE,
        &COLUMNS,
        "id",
        &["id", "amount", "owner", "touched"],
        &[],
        &["id", "amount"],
        read_allow,
        read_deny,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        None,
        false,
        &[],
        &[],
        None,
        None,
        &[],
    )))
}

pub(super) async fn reseed(pool: &PgPool) {
    sqlx::query("DROP TABLE IF EXISTS bigint_policy_rows")
        .execute(pool)
        .await
        .expect("drop");
    sqlx::query(
        "CREATE TABLE bigint_policy_rows (id BIGINT PRIMARY KEY, amount BIGINT NOT NULL, \
         owner BIGINT, touched BOOLEAN NOT NULL DEFAULT FALSE)",
    )
    .execute(pool)
    .await
    .expect("create");
    let mut insert = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "INSERT INTO bigint_policy_rows (id, amount, owner) VALUES ",
    );
    for (index, value) in ALL.iter().enumerate() {
        if index > 0 {
            insert.push(", ");
        }
        insert.push("(");
        push_bind_value(&mut insert, &SqlValue::BigInt(*value));
        insert.push(", ");
        push_bind_value(&mut insert, &SqlValue::BigInt(*value));
        insert.push(", ");
        push_bind_value(&mut insert, &SqlValue::NullBigInt);
        insert.push(")");
    }
    insert.build().execute(pool).await.expect("insert");
}

pub(super) fn all_except(excluded: &[i64]) -> Vec<i64> {
    let mut rest: Vec<i64> = ALL
        .iter()
        .copied()
        .filter(|value| !excluded.contains(value))
        .collect();
    rest.sort_unstable();
    rest
}

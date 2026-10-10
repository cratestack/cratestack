//! `SqlValue::BigInt` / `NullBigInt` through `push_bind_value`, and the
//! shape of what the sqlx impls on `cratestack_core::BigInt` give a
//! `BigInt @id` or foreign key. The values that reach a real column are
//! checked against Postgres in `pg`.

use cratestack_core::BigInt;

use super::BOUNDARIES;
use crate::query::push_bind_value;
use crate::sqlx::{Arguments as _, Execute as _};
use crate::{SqlValue, sqlx};

fn bound(values: &[SqlValue]) -> (String, usize) {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        push_bind_value(&mut query, value);
    }
    let sql = query.sql().as_str().to_owned();
    let arguments = query
        .build()
        .take_arguments()
        .expect("arguments encode")
        .expect("arguments present");
    (sql, arguments.len())
}

#[test]
fn bigint_and_null_bigint_each_take_one_bind_slot() {
    let mut values: Vec<SqlValue> = BOUNDARIES.iter().map(|v| SqlValue::BigInt(*v)).collect();
    values.push(SqlValue::NullBigInt);
    let (sql, count) = bound(&values);
    assert_eq!(sql, "SELECT $1, $2, $3, $4, $5, $6");
    assert_eq!(count, 6);
}

/// A `BigInt` and an `Int` of the same number are different variants. The
/// create path compares them numerically on purpose (`comparison.rs`); derived
/// equality, used by batch-upsert deduplication over one column's values,
/// still tells them apart.
#[test]
fn bigint_is_not_int_to_derived_equality() {
    assert_ne!(SqlValue::BigInt(7), SqlValue::Int(7));
    assert_eq!(SqlValue::BigInt(7), SqlValue::BigInt(7));
    assert_ne!(SqlValue::NullBigInt, SqlValue::NullInt);
}

/// D-PK: a `BigInt` is a Postgres scalar and its `Option` and slice forms
/// bind, which is what the delegates' `PK: Type + Encode` bounds ask. The
/// foreign-key side (`find_many_with(..)`'s `RelPK`) adds `Clone + Eq + Hash +
/// IntoSqlValue`, mirrored here bound for bound.
#[test]
fn a_bigint_satisfies_the_delegate_primary_and_foreign_key_bounds() {
    fn pk<PK>(_: PK)
    where
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
    }
    fn rel_pk<RelPK>(_: RelPK)
    where
        RelPK: Send
            + Clone
            + Eq
            + std::hash::Hash
            + cratestack_sql::IntoSqlValue
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
    }
    pk(BigInt::new(7));
    pk(Some(BigInt::new(7)));
    pk(vec![BigInt::new(1), BigInt::new(2)]);
    rel_pk(BigInt::new(7));
}

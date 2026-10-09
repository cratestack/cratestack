//! `BigInt` through the SQLite value layer (ADR 0019 PR B, package B3), on a
//! real in-memory database: bound as an INTEGER, read back exact at the
//! `i64` edges and at 2^53 + 1, and filtered with `!=`, `in`, `not in` and
//! the comparison operators. Policies do not exist on the embedded backend
//! (`render.rs`), so the SQLite counterpart of "a negated rule denies" is
//! that a negated filter excludes exactly the matching row.

#![cfg(test)]

use cratestack_core::BigInt;
use cratestack_sql::{FieldRef, FilterExpr, ModelColumn, ModelDescriptor, SqlValue, SqliteDialect};
use rusqlite::{Connection, params_from_iter};

use super::SqlValueParam;
use crate::render_select;

const BOUNDARIES: [i64; 5] = [i64::MIN, -1, 0, 9_007_199_254_740_993, i64::MAX];

fn connection_with_rows() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, amount BLOB, owner BLOB)")
        .unwrap();
    for value in BOUNDARIES {
        conn.execute(
            "INSERT INTO t (id, amount, owner) VALUES (?1, ?2, ?3)",
            [
                SqlValueParam(&SqlValue::BigInt(value)),
                SqlValueParam(&SqlValue::BigInt(value)),
                SqlValueParam(&SqlValue::NullBigInt),
            ],
        )
        .unwrap();
    }
    conn
}

fn descriptor() -> ModelDescriptor<(), BigInt> {
    const COLUMNS: &[ModelColumn] = &[
        ModelColumn {
            rust_name: "id",
            sql_name: "id",
        },
        ModelColumn {
            rust_name: "amount",
            sql_name: "amount",
        },
    ];
    ModelDescriptor::new(
        "Entry",
        "t",
        COLUMNS,
        "id",
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
    )
}

/// Run the crate's own `SELECT` renderer and execute it, so the filter text and
/// the bind order are the ones a delegate would use.
fn amounts_where(conn: &Connection, filter: FilterExpr) -> Vec<i64> {
    let (sql, binds) = render_select(
        &SqliteDialect,
        &descriptor(),
        &[filter],
        &[FieldRef::<(), BigInt>::new("amount").asc()],
        None,
        None,
    );
    let mut statement = conn.prepare(&sql).expect(&sql);
    statement
        .query_map(params_from_iter(binds.iter().map(SqlValueParam)), |row| {
            Ok(row.get::<_, BigInt>(1)?.get())
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn all_except(excluded: &[i64]) -> Vec<i64> {
    let mut rest: Vec<i64> = BOUNDARIES
        .iter()
        .copied()
        .filter(|v| !excluded.contains(v))
        .collect();
    rest.sort_unstable();
    rest
}

/// D-PK on the embedded side: the delegates bound a key by `IntoSqlValue +
/// Clone` (and `Eq + Hash` for a foreign key), which a `BigInt` meets.
#[test]
fn a_bigint_satisfies_the_delegate_key_bounds() {
    fn key<PK: cratestack_sql::IntoSqlValue + Clone>(_: PK) {}
    fn rel_key<RelPK: Clone + Eq + std::hash::Hash + cratestack_sql::IntoSqlValue>(_: RelPK) {}
    key(BigInt::new(7));
    rel_key(BigInt::new(7));
}

#[test]
fn a_bigint_is_stored_as_a_sqlite_integer_and_read_back_exact() {
    let conn = connection_with_rows();
    let mut statement = conn
        .prepare("SELECT id, amount, typeof(amount), owner FROM t ORDER BY id")
        .unwrap();
    let rows: Vec<(BigInt, BigInt, String, Option<BigInt>)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let ids: Vec<i64> = rows.iter().map(|row| row.0.get()).collect();
    assert_eq!(ids, all_except(&[]));
    for (id, amount, class, owner) in rows {
        assert_eq!(id, amount);
        assert_eq!(class, "integer");
        assert!(owner.is_none(), "NullBigInt is NULL");
    }
}

/// A column some other writer filled with a REAL, TEXT or BLOB is refused on
/// read, never coerced.
#[test]
fn a_non_integer_stored_value_is_refused_on_read() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE t (x BLOB); INSERT INTO t VALUES (1.5), ('7'), (x'0102'), (NULL);",
    )
    .unwrap();
    let mut statement = conn.prepare("SELECT x FROM t").unwrap();
    let outcomes: Vec<bool> = statement
        .query_map([], |row| Ok(row.get::<_, BigInt>(0).is_err()))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(outcomes, vec![true, true, true, true]);
}

#[test]
fn a_negated_bigint_filter_excludes_exactly_the_matching_row() {
    let conn = connection_with_rows();
    let amount = FieldRef::<(), BigInt>::new("amount");
    for value in BOUNDARIES {
        let ne = FilterExpr::from(amount.ne(BigInt::new(value)));
        assert_eq!(amounts_where(&conn, ne), all_except(&[value]), "!= {value}");
        let eq = FilterExpr::from(amount.eq(BigInt::new(value)));
        assert_eq!(amounts_where(&conn, eq), vec![value], "== {value}");
        let not_eq = FilterExpr::from(amount.eq(BigInt::new(value))).not();
        assert_eq!(amounts_where(&conn, not_eq), all_except(&[value]));
    }
}

#[test]
fn in_and_not_in_and_ordering_filters_are_exact_at_the_edges() {
    let conn = connection_with_rows();
    let amount = FieldRef::<(), BigInt>::new("amount");
    let set = [i64::MIN, 9_007_199_254_740_993, i64::MAX];
    let is_in = FilterExpr::from(amount.in_(set.map(BigInt::new)));
    assert_eq!(amounts_where(&conn, is_in.clone()), all_except(&[-1, 0]));
    assert_eq!(amounts_where(&conn, is_in.not()), all_except(&set));

    let beyond_js = BigInt::new(9_007_199_254_740_992);
    let gt = FilterExpr::from(amount.gt(beyond_js));
    assert_eq!(
        amounts_where(&conn, gt),
        vec![9_007_199_254_740_993, i64::MAX],
        "2^53 + 1 is greater than 2^53: no float rounding in the comparison"
    );
    let lt = FilterExpr::from(amount.lt(BigInt::new(i64::MIN + 1)));
    assert_eq!(amounts_where(&conn, lt), vec![i64::MIN]);
}

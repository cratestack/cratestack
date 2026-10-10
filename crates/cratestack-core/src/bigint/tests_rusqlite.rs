//! SQLite integer storage for `BigInt`, through a real in-memory database.

use rusqlite::Connection;

use super::BigInt;
use super::tests::BOUNDARIES;

fn connection() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    // BLOB affinity, as the embedded backend declares every column.
    connection
        .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, v BLOB);")
        .unwrap();
    connection
}

#[test]
fn boundary_values_round_trip_as_sqlite_integers() {
    let connection = connection();
    for (index, (value, _)) in BOUNDARIES.into_iter().enumerate() {
        let id = i64::try_from(index).unwrap();
        connection
            .execute(
                "INSERT INTO t (id, v) VALUES (?1, ?2)",
                (id, BigInt::new(value)),
            )
            .unwrap();
        let (read, storage): (BigInt, String) = connection
            .query_row("SELECT v, typeof(v) FROM t WHERE id = ?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(read, BigInt::new(value));
        assert_eq!(storage, "integer", "{value} must be stored as an integer");
    }
}

#[test]
fn a_big_int_binds_as_a_primary_key_lookup() {
    let connection = connection();
    connection
        .execute("INSERT INTO t (id, v) VALUES (?1, 'x')", [BigInt::MAX])
        .unwrap();
    let found: Option<i64> = connection
        .query_row("SELECT id FROM t WHERE id = ?1", [BigInt::MAX], |row| {
            row.get(0)
        })
        .ok();
    assert_eq!(found, Some(i64::MAX));
}

#[test]
fn text_and_real_values_are_refused_not_coerced() {
    let connection = connection();
    for literal in ["'5'", "5.0", "x'05'", "NULL"] {
        let result = connection.query_row(&format!("SELECT {literal}"), [], |row| {
            row.get::<_, BigInt>(0)
        });
        assert!(result.is_err(), "{literal} must not read as a BigInt");
    }
}

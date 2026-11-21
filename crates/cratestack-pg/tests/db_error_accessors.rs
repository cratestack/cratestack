//! cratestack#745 (a24-context#745): a database error read back through the
//! typed accessors keeps its SQLSTATE, constraint, table and column, whatever
//! raised it: a CHECK, NOT NULL, FK or named UNIQUE constraint, or a trigger
//! raising a custom or `restrict_violation` code. The generated model's
//! create path is the call site for those; the concurrency and timeout
//! classes go through `cratestack_error_from_sqlx`, the conversion every
//! generated and hand-written sqlx call site shares.

use cratestack::include_server_schema;
use cratestack::sqlx::{Connection, PgConnection, query};
use cratestack::{CratestackContext, CratestackError, Value, cratestack_error_from_sqlx};

include_server_schema!("tests/fixtures/db_error_accessors.cstack", db = Postgres);

mod support;

use support::pg;

const SETUP: &str = "
DROP TABLE IF EXISTS cratestack_event_outbox, parcels, locks_745, ssi_745, parted_745 CASCADE;
DROP FUNCTION IF EXISTS parcels_guard_745();
CREATE TABLE parcels (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    code TEXT NOT NULL CONSTRAINT parcels_code_once UNIQUE,
    qty BIGINT NOT NULL CONSTRAINT parcels_qty_nonneg CHECK (qty >= 0),
    note TEXT NOT NULL,
    parent_id BIGINT CONSTRAINT parcels_parent_fk REFERENCES parcels (id)
);
CREATE FUNCTION parcels_guard_745() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.name = 'trip-a2401' THEN
        RAISE EXCEPTION 'reservation refused' USING ERRCODE = 'A2401',
            TABLE = TG_TABLE_NAME, SCHEMA = TG_TABLE_SCHEMA;
    ELSIF NEW.name = 'trip-restrict' THEN
        RAISE EXCEPTION 'append only' USING ERRCODE = 'restrict_violation',
            TABLE = TG_TABLE_NAME, SCHEMA = TG_TABLE_SCHEMA;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER parcels_guard BEFORE INSERT ON parcels
    FOR EACH ROW EXECUTE FUNCTION parcels_guard_745();
CREATE TABLE locks_745 (id INT PRIMARY KEY);
INSERT INTO locks_745 VALUES (1), (2);
CREATE TABLE ssi_745 (id INT PRIMARY KEY);
CREATE TABLE parted_745 (d DATE NOT NULL) PARTITION BY RANGE (d);
CREATE TABLE parted_745_2026 PARTITION OF parted_745
    FOR VALUES FROM ('2026-01-01') TO ('2027-01-01');
";

fn ctx() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

fn input(id: i64, name: &str, code: &str, qty: i64) -> cratestack_schema::CreateParcelInput {
    cratestack_schema::CreateParcelInput {
        id,
        name: name.to_owned(),
        code: code.to_owned(),
        qty,
        note: Some("n".to_owned()),
        parent_id: None,
    }
}

async fn reset(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::raw_sql(SETUP)
        .execute(pool)
        .await
        .expect("setup");
}

#[track_caller]
fn assert_db(
    err: &CratestackError,
    sqlstate: &str,
    constraint: Option<&str>,
    table: Option<&str>,
    column: Option<&str>,
) {
    assert_eq!(err.db_sqlstate(), Some(sqlstate), "{err:?}");
    assert_eq!(err.db_constraint(), constraint, "{err:?}");
    assert_eq!(err.db_table(), table, "{err:?}");
    assert_eq!(err.db_column(), column, "{err:?}");
    assert!(err.db_message().is_some_and(|m| !m.is_empty()), "{err:?}");
}

#[tokio::test]
async fn generated_model_errors_keep_sqlstate_constraint_table_and_column() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let parcels = || cool.parcel();

    parcels()
        .create(input(1, "ok", "c1", 1))
        .run(&ctx())
        .await
        .expect("seed");

    // CHECK: 23514 with the constraint name.
    let err = parcels()
        .create(input(2, "ok", "c2", -1))
        .run(&ctx())
        .await
        .unwrap_err();
    assert_db(
        &err,
        "23514",
        Some("parcels_qty_nonneg"),
        Some("parcels"),
        None,
    );
    assert_eq!(err.db_schema(), Some("public"));

    // NOT NULL: 23502, no constraint, the column is named.
    let mut missing_note = input(3, "ok", "c3", 1);
    missing_note.note = None;
    let err = parcels()
        .create(missing_note)
        .run(&ctx())
        .await
        .unwrap_err();
    assert_db(&err, "23502", None, Some("parcels"), Some("note"));

    // FK: 23503 with the constraint name.
    let mut orphan = input(4, "ok", "c4", 1);
    orphan.parent_id = Some(999);
    let err = parcels().create(orphan).run(&ctx()).await.unwrap_err();
    assert_db(
        &err,
        "23503",
        Some("parcels_parent_fk"),
        Some("parcels"),
        None,
    );

    // Named UNIQUE: a typed conflict, 409, with the constraint name and the
    // same message the generic path would carry.
    let err = parcels()
        .create(input(5, "ok", "c1", 1))
        .run(&ctx())
        .await
        .unwrap_err();
    assert!(matches!(err, CratestackError::ConflictTyped(_)), "{err:?}");
    assert_db(
        &err,
        "23505",
        Some("parcels_code_once"),
        Some("parcels"),
        None,
    );

    // Triggers: a custom code and `restrict_violation` pass through unchanged.
    let err = parcels()
        .create(input(6, "trip-a2401", "c6", 1))
        .run(&ctx())
        .await
        .unwrap_err();
    assert_db(&err, "A2401", None, Some("parcels"), None);
    assert_eq!(err.db_message(), Some("reservation refused"));
    let err = parcels()
        .create(input(7, "trip-restrict", "c7", 1))
        .run(&ctx())
        .await
        .unwrap_err();
    assert_db(&err, "23001", None, Some("parcels"), None);

    // `Display` is unchanged: it still prints the server message.
    assert_eq!(err.to_string(), "database: append only");
}

#[tokio::test]
async fn partition_miss_is_23514_with_table_and_no_constraint() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    let err = query("INSERT INTO parted_745 VALUES ('2030-01-01')")
        .execute(&test_pg.pool)
        .await
        .map_err(cratestack_error_from_sqlx)
        .unwrap_err();
    assert_db(&err, "23514", None, Some("parted_745"), None);
}

/// `40001`, `40P01`, `55P03` and `57014` are what a worker retries on; they
/// come from two real connections, not a synthetic error.
#[tokio::test]
async fn concurrency_and_timeout_classes_keep_their_sqlstate() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    let conn = || async { PgConnection::connect(&test_pg.url).await.expect("connect") };
    let map = |e| cratestack_error_from_sqlx(e);

    // 40001: two serializable transactions each read the empty table and
    // insert; the second to commit is the serialization failure.
    let (mut a, mut b) = (conn().await, conn().await);
    let mut failure = None;
    for c in [&mut a, &mut b] {
        query("BEGIN ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *c)
            .await
            .unwrap();
        query("SELECT count(*) FROM ssi_745")
            .fetch_one(&mut *c)
            .await
            .unwrap();
    }
    for (c, id) in [(&mut a, 1), (&mut b, 2)] {
        let step = async {
            query("INSERT INTO ssi_745 VALUES ($1)")
                .bind(id)
                .execute(&mut *c)
                .await?;
            query("COMMIT").execute(&mut *c).await
        };
        if let Err(e) = step.await {
            failure = Some(map(e));
        }
    }
    assert_db(
        &failure.expect("a serialization failure"),
        "40001",
        None,
        None,
        None,
    );

    // 40P01: each connection locks one row, then reaches for the other's.
    let (mut a, mut b) = (conn().await, conn().await);
    query("BEGIN").execute(&mut a).await.unwrap();
    query("BEGIN").execute(&mut b).await.unwrap();
    query("SELECT 1 FROM locks_745 WHERE id = 1 FOR UPDATE")
        .execute(&mut a)
        .await
        .unwrap();
    query("SELECT 1 FROM locks_745 WHERE id = 2 FOR UPDATE")
        .execute(&mut b)
        .await
        .unwrap();
    let (ra, rb) = tokio::join!(
        query("SELECT 1 FROM locks_745 WHERE id = 2 FOR UPDATE").execute(&mut a),
        query("SELECT 1 FROM locks_745 WHERE id = 1 FOR UPDATE").execute(&mut b),
    );
    let deadlock = ra
        .err()
        .or(rb.err())
        .map(map)
        .expect("one side is the deadlock victim");
    assert_db(&deadlock, "40P01", None, None, None);
    drop((a, b));

    // 55P03: a held row lock and `SET LOCAL lock_timeout`.
    let (mut holder, mut waiter) = (conn().await, conn().await);
    query("BEGIN").execute(&mut holder).await.unwrap();
    query("SELECT 1 FROM locks_745 WHERE id = 1 FOR UPDATE")
        .execute(&mut holder)
        .await
        .unwrap();
    query("BEGIN").execute(&mut waiter).await.unwrap();
    query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut waiter)
        .await
        .unwrap();
    let err = query("SELECT 1 FROM locks_745 WHERE id = 1 FOR UPDATE")
        .execute(&mut waiter)
        .await
        .map_err(map)
        .unwrap_err();
    assert_db(&err, "55P03", None, None, None);
    drop((holder, waiter));

    // 57014: `SET LOCAL statement_timeout` and a long sleep.
    let mut c = conn().await;
    query("BEGIN").execute(&mut c).await.unwrap();
    query("SET LOCAL statement_timeout = '100ms'")
        .execute(&mut c)
        .await
        .unwrap();
    let err = query("SELECT pg_sleep(5)")
        .execute(&mut c)
        .await
        .map_err(map)
        .unwrap_err();
    assert_db(&err, "57014", None, None, None);
}

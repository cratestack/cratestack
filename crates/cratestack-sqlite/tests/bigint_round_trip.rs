//! ADR 0019 PR B (B11): the `BigInt` round trip through the embedded backend,
//! natively (rusqlite, in memory). Real SQLite, no skip path.
//!
//! The values are the ADR's: `i64::MAX`, `i64::MIN` and `2^53 + 1`. SQLite has
//! no native 64-bit type distinct from its INTEGER, so what could go wrong is
//! the bind: a `BigInt` written as text, or read back through `f64`, would be
//! stored and compared wrongly with no error. Each test reads the column back
//! with raw SQL (`typeof`, and the exact `i64`) rather than trusting the
//! generated decoder alone.

use cratestack::include_embedded_schema;
use cratestack::rusqlite;
use cratestack::{BigInt, CreateModelInput, RusqliteRuntime};
use cratestack_rusqlite::{ModelDelegate, RusqliteError, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/bigint_round_trip.cstack");

use cratestack_schema::models::{BiLite, BiLiteLimit, BiLitePlayer, BiLiteTeam};
use cratestack_schema::{
    BI_LITE_LIMIT_MODEL, BI_LITE_MODEL, BI_LITE_PLAYER_MODEL, BI_LITE_TEAM_MODEL,
    CreateBiLiteInput, CreateBiLiteLimitInput, CreateBiLitePlayerInput, CreateBiLiteTeamInput,
    UpdateBiLiteInput, bi_lite, bi_lite_player,
};

const BOUNDARY: [i64; 3] = [i64::MAX, i64::MIN, 9_007_199_254_740_993];
const TWO_53: i64 = 9_007_199_254_740_992;
const TWO_53_PLUS_2: i64 = 9_007_199_254_740_994;

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let ddl = [
        create_table_sql(&BI_LITE_MODEL),
        create_table_sql(&BI_LITE_LIMIT_MODEL),
        create_table_sql(&BI_LITE_TEAM_MODEL),
        create_table_sql(&BI_LITE_PLAYER_MODEL),
    ]
    .join(";\n");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&format!("{ddl};")).expect("apply DDL");
            Ok(())
        })
        .expect("connection");
    runtime
}

fn accounts(runtime: &RusqliteRuntime) -> ModelDelegate<'_, BiLite, BigInt> {
    ModelDelegate::<BiLite, BigInt>::new(runtime, &BI_LITE_MODEL)
}

fn create_input(id: i64, fee: Option<i64>) -> CreateBiLiteInput {
    CreateBiLiteInput {
        id: BigInt::new(id),
        amountE8: BigInt::new(id),
        feeE8: fee.map(BigInt::new),
        label: "x".to_owned(),
    }
}

/// `(typeof(col), col)` of one row, read with raw SQL and the exact `i64`.
fn raw(runtime: &RusqliteRuntime, column: &str, id: i64) -> (String, Option<i64>) {
    runtime
        .with_connection(|conn| {
            Ok(conn.query_row(
                &format!("SELECT typeof({column}), {column} FROM bi_lites WHERE id = ?1"),
                rusqlite::params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
            )?)
        })
        .expect("raw read")
}

fn seed_five(runtime: &RusqliteRuntime) {
    let delegate = accounts(runtime);
    for (id, fee) in [
        (i64::MIN, None),
        (TWO_53, Some(TWO_53)),
        (BOUNDARY[2], Some(BOUNDARY[2])),
        (TWO_53_PLUS_2, None),
        (i64::MAX, Some(i64::MAX)),
    ] {
        delegate
            .create(create_input(id, fee))
            .run()
            .expect("seed row");
    }
}

fn ids(rows: &[BiLite]) -> Vec<i64> {
    rows.iter().map(|row| row.id.get()).collect()
}

#[test]
fn the_ddl_does_not_declare_a_bigint_column_as_text() {
    let ddl = create_table_sql(&BI_LITE_MODEL);
    for column in ["amount_e8", "fee_e8"] {
        let line = ddl
            .lines()
            .find(|line| line.contains(column))
            .unwrap_or_else(|| panic!("no {column} in {ddl}"));
        assert!(
            !line.to_uppercase().contains("TEXT"),
            "{column} is declared TEXT: {line}"
        );
    }
}

#[test]
fn create_find_update_delete_keep_the_exact_i64_stored_as_an_integer() {
    let runtime = setup();
    let delegate = accounts(&runtime);

    for n in BOUNDARY {
        let id = BigInt::new(n);
        let created = delegate
            .create(create_input(n, Some(n)))
            .run()
            .expect("create with a BigInt key");
        assert_eq!(created.id, id);
        assert_eq!(created.amountE8, id);
        assert_eq!(created.feeE8, Some(id));

        // The bind: INTEGER storage and the exact value, in every BigInt column.
        for column in ["id", "amount_e8", "fee_e8"] {
            assert_eq!(
                raw(&runtime, column, n),
                ("integer".to_owned(), Some(n)),
                "{column} of {n}: a BigInt must be bound as an integer, not text or a real"
            );
        }

        let found = delegate
            .find_unique(id)
            .run()
            .expect("find_unique")
            .expect("row exists");
        assert_eq!(found, created);

        // Update to another boundary value, and clear the optional one.
        let other = BOUNDARY[(BOUNDARY.iter().position(|b| *b == n).unwrap() + 1) % 3];
        let updated = delegate
            .update(id)
            .set(UpdateBiLiteInput {
                amountE8: Some(BigInt::new(other)),
                feeE8: Some(None),
                ..Default::default()
            })
            .run()
            .expect("update by a BigInt key");
        assert_eq!(updated.amountE8, BigInt::new(other));
        assert_eq!(updated.feeE8, None);
        assert_eq!(
            raw(&runtime, "amount_e8", n),
            ("integer".to_owned(), Some(other))
        );
        assert_eq!(raw(&runtime, "fee_e8", n), ("null".to_owned(), None));

        let deleted = delegate.delete(id).run().expect("delete by a BigInt key");
        assert_eq!(deleted.id, id);
        assert!(delegate.find_unique(id).run().expect("find").is_none());
    }
}

#[test]
fn a_bigint_primary_key_conflict_is_a_conflict() {
    let runtime = setup();
    let delegate = accounts(&runtime);
    delegate
        .create(create_input(i64::MAX, None))
        .run()
        .expect("first");
    let error = delegate
        .create(create_input(i64::MAX, None))
        .run()
        .expect_err("the second insert collides on the primary key");
    assert!(
        matches!(
            &error,
            RusqliteError::Sqlite(rusqlite::Error::SqliteFailure(failure, _))
                if failure.code == rusqlite::ErrorCode::ConstraintViolation
        ),
        "expected a constraint violation, got {error:?}"
    );
}

#[test]
fn gt_gte_lt_lte_eq_ne_and_in_compare_numerically_at_the_boundaries() {
    let runtime = setup();
    seed_five(&runtime);
    let delegate = accounts(&runtime);
    let find = |filter: cratestack::Filter| {
        ids(&delegate
            .find_many()
            .where_(filter)
            .order_by(bi_lite::id().asc())
            .run()
            .expect("filtered find_many"))
    };
    let above = BigInt::new(BOUNDARY[2]);

    assert_eq!(
        find(bi_lite::amountE8().gt(above)),
        vec![TWO_53_PLUS_2, i64::MAX]
    );
    assert_eq!(
        find(bi_lite::amountE8().gte(above)),
        vec![BOUNDARY[2], TWO_53_PLUS_2, i64::MAX]
    );
    assert_eq!(find(bi_lite::amountE8().lt(above)), vec![i64::MIN, TWO_53]);
    assert_eq!(
        find(bi_lite::amountE8().lte(above)),
        vec![i64::MIN, TWO_53, BOUNDARY[2]]
    );
    assert_eq!(find(bi_lite::amountE8().eq(above)), vec![BOUNDARY[2]]);
    assert_eq!(
        find(bi_lite::amountE8().ne(above)),
        vec![i64::MIN, TWO_53, TWO_53_PLUS_2, i64::MAX]
    );
    assert_eq!(
        find(bi_lite::amountE8().in_([BigInt::MAX, BigInt::MIN])),
        vec![i64::MIN, i64::MAX]
    );
    assert_eq!(find(bi_lite::id().gt(above)), vec![TWO_53_PLUS_2, i64::MAX]);
    assert_eq!(
        find(bi_lite::amountE8().gt(BigInt::new(i64::MAX - 1))),
        vec![i64::MAX]
    );
    assert_eq!(
        find(bi_lite::amountE8().lt(BigInt::new(i64::MIN + 1))),
        vec![i64::MIN]
    );
    assert_eq!(
        find(bi_lite::feeE8().is_null()),
        vec![i64::MIN, TWO_53_PLUS_2]
    );
    assert_eq!(
        find(bi_lite::feeE8().gt(BigInt::new(TWO_53))),
        vec![BOUNDARY[2], i64::MAX]
    );
}

#[test]
fn ordering_by_a_bigint_column_is_numeric_not_lexical() {
    let runtime = setup();
    seed_five(&runtime);
    let delegate = accounts(&runtime);
    let desc = delegate
        .find_many()
        .order_by(bi_lite::amountE8().desc())
        .run()
        .expect("order by");
    assert_eq!(
        ids(&desc),
        vec![i64::MAX, TWO_53_PLUS_2, BOUNDARY[2], TWO_53, i64::MIN]
    );
}

#[test]
fn batch_operations_bind_bigint_keys() {
    let runtime = setup();
    let delegate = accounts(&runtime);
    let created = delegate
        .batch_create(vec![
            create_input(i64::MAX, None),
            create_input(i64::MIN, None),
            create_input(BOUNDARY[2], None),
        ])
        .run()
        .expect("batch_create");
    assert_eq!((created.summary.ok, created.summary.err), (3, 0));

    let got = delegate
        .batch_get(vec![BigInt::MAX, BigInt::new(7), BigInt::MIN])
        .run()
        .expect("batch_get");
    assert_eq!((got.summary.ok, got.summary.err), (2, 1));
    assert_eq!(got.results[1].index, 1);

    // A colliding BigInt key fails its own item and not the batch.
    let mixed = delegate
        .batch_create(vec![create_input(i64::MAX, None), create_input(5, None)])
        .run()
        .expect("batch_create with a collision");
    assert_eq!((mixed.summary.ok, mixed.summary.err), (1, 1));

    // upsert on a BigInt primary key updates in place.
    let mut input = create_input(i64::MAX, None);
    input.label = "again".to_owned();
    let upserted = delegate.upsert(input).run().expect("upsert");
    assert_eq!(upserted.label, "again");
    assert_eq!(
        delegate
            .find_many()
            .where_(bi_lite::id().eq(BigInt::MAX))
            .run()
            .expect("find")
            .len(),
        1,
        "the upsert must not duplicate the row"
    );
}

#[test]
fn range_on_a_bigint_is_enforced_at_both_inclusive_bounds() {
    // The embedded runtime validates through the generated input's `validate()`
    // (it does not call it from `run()`), as `nullable_update_validator.rs` does.
    let make = |id: i64, small: i64, wide: i64| CreateBiLiteLimitInput {
        id,
        smallE8: BigInt::new(small),
        wideE8: BigInt::new(wide),
    };
    for (small, wide) in [
        (0, i64::MIN + 1),
        (1_000_000, i64::MAX - 1),
        (999_999, BOUNDARY[2]),
    ] {
        make(1, small, wide)
            .validate()
            .unwrap_or_else(|e| panic!("({small}, {wide}) is inside both ranges: {e:?}"));
    }
    for (small, wide, field) in [
        (-1, 0, "smallE8"),
        (1_000_001, 0, "smallE8"),
        (BOUNDARY[2], 0, "smallE8"),
        (i64::MAX, 0, "smallE8"),
        (0, i64::MIN, "wideE8"),
        (0, i64::MAX, "wideE8"),
    ] {
        let error = make(1, small, wide)
            .validate()
            .expect_err("outside the range");
        assert_eq!(error.code(), "VALIDATION_ERROR", "({small}, {wide})");
        assert!(
            error.public_message().contains(field),
            "({small}, {wide}): the message must name `{field}`: {}",
            error.public_message()
        );
    }

    // And the rows that pass validation round-trip through the delegate.
    let runtime = setup();
    let delegate = ModelDelegate::<BiLiteLimit, i64>::new(&runtime, &BI_LITE_LIMIT_MODEL);
    let created = delegate
        .create(make(1, 0, i64::MIN + 1))
        .run()
        .expect("create inside the range");
    assert_eq!(
        created.wideE8,
        BigInt::MIN
            .checked_add(BigInt::new(1))
            .expect("no overflow")
    );
}

#[test]
fn a_bigint_foreign_key_is_stored_as_an_integer_and_filters() {
    let runtime = setup();
    let teams = ModelDelegate::<BiLiteTeam, BigInt>::new(&runtime, &BI_LITE_TEAM_MODEL);
    let players = ModelDelegate::<BiLitePlayer, BigInt>::new(&runtime, &BI_LITE_PLAYER_MODEL);
    for (id, name) in [(i64::MAX, "max"), (BOUNDARY[2], "mid")] {
        teams
            .create(CreateBiLiteTeamInput {
                id: BigInt::new(id),
                name: name.to_owned(),
            })
            .run()
            .expect("team");
    }
    for (id, team) in [(i64::MIN, i64::MAX), (TWO_53, i64::MAX), (1, BOUNDARY[2])] {
        let player = players
            .create(CreateBiLitePlayerInput {
                id: BigInt::new(id),
                teamId: BigInt::new(team),
                scoreE8: BigInt::new(id),
            })
            .run()
            .expect("player");
        assert_eq!(player.teamId, BigInt::new(team));
    }
    let stored: (String, i64) = runtime
        .with_connection(|conn| {
            Ok(conn.query_row(
                "SELECT typeof(team_id), team_id FROM bi_lite_players WHERE id = ?1",
                rusqlite::params![i64::MIN],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?)
        })
        .expect("raw");
    assert_eq!(stored, ("integer".to_owned(), i64::MAX));

    let on_max = players
        .find_many()
        .where_(bi_lite_player::teamId().eq(BigInt::MAX))
        .order_by(bi_lite_player::id().asc())
        .run()
        .expect("filter by the foreign key");
    assert_eq!(
        on_max.iter().map(|p| p.id.get()).collect::<Vec<_>>(),
        vec![i64::MIN, TWO_53]
    );
}

#[test]
fn a_bigint_serializes_as_a_string_through_the_generated_structs() {
    // The embedded structs are what a hybrid app sends to a server, so they must
    // carry the same wire form (ADR 0019 D2) even though SQLite never sees it.
    let row = BiLite {
        id: BigInt::MAX,
        amountE8: BigInt::MIN,
        feeE8: Some(BigInt::new(BOUNDARY[2])),
        label: "x".to_owned(),
    };
    let json = serde_json::to_value(&row).expect("serialize");
    assert_eq!(json["id"], "9223372036854775807");
    assert_eq!(json["amountE8"], "-9223372036854775808");
    assert_eq!(json["feeE8"], "9007199254740993");
    let back: BiLite = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back, row);
    assert!(
        serde_json::from_value::<BiLite>(serde_json::json!({
            "id": 1, "amountE8": "1", "feeE8": null, "label": "x"
        }))
        .is_err()
    );
}

//! ADR 0019 PR B (B11): the model-level behaviours of `BigInt` that need more
//! than a column: `@version`, `@range ... @db_enforce`, and a `BigInt` foreign
//! key with a relation include. Same fixture and server as
//! `bigint_end_to_end.rs` (see its header), real Postgres, JSON and CBOR.

use cratestack::axum::Router;
use cratestack::axum::http::StatusCode;
use cratestack::serde_json::{Value as Json, json};
use cratestack::sqlx::PgPool;
use cratestack::{CodecSet, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

include_server_schema!("tests/fixtures/bigint_end_to_end.cstack", db = Postgres);

mod bigint_end_to_end_support;
mod bigint_support;
mod support;

use bigint_end_to_end_support::Procedures;
use bigint_support::{
    ABOVE_2_53, I64_MAX, I64_MIN, Wire, call, db_i64, has_cbor_text, operator_auth, reset,
};
use support::pg;

const SCHEMA: &str = include_str!("fixtures/bigint_end_to_end.cstack");
const TABLES: &str = "bi_players, bi_teams, bi_limits, bi_ledgers, bi_accounts";
const TWO_53: &str = "9007199254740992";

fn router(pool: &PgPool) -> Router {
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        Procedures,
        (),
        CodecSet::new(CborCodec, JsonCodec),
        operator_auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

macro_rules! fixture {
    ($test_pg:ident, $router:ident) => {
        let _guard = pg::serial_guard().await;
        let Some($test_pg) = pg::connect_or_skip().await else {
            return;
        };
        reset(&$test_pg.pool, SCHEMA, TABLES).await;
        let $router = router(&$test_pg.pool);
    };
}

fn text(value: &Json) -> &str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("a BigInt is a string on the wire, got {value}"))
}

// ---------------------------------------------------------------------------
// @version on BigInt: the ETag / If-Match round trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_bigint_version_round_trips_through_etag_and_if_match() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    for (wire, id) in [(Wire::Json, 1_i64), (Wire::Cbor, 2_i64)] {
        let path = format!("/bi_ledgers/{id}");
        let created = call(
            &router,
            "POST",
            "/bi_ledgers",
            wire,
            &[],
            Some(&json!({"id": id.to_string(), "balanceE8": "5"})),
        )
        .await
        .expect(StatusCode::CREATED);
        assert_eq!(
            created.json()["version"],
            Json::String("0".to_owned()),
            "{wire:?}: a BigInt version is a string in the body"
        );

        let got = call(&router, "GET", &path, wire, &[], None)
            .await
            .expect(StatusCode::OK);
        assert_eq!(got.etag().as_deref(), Some("\"0\""));

        call(
            &router,
            "PATCH",
            &path,
            wire,
            &[],
            Some(&json!({"balanceE8": "6"})),
        )
        .await
        .expect(StatusCode::PRECONDITION_FAILED);
        call(
            &router,
            "PATCH",
            &path,
            wire,
            &[("if-match", "\"abc\"")],
            Some(&json!({"balanceE8": "6"})),
        )
        .await
        .expect(StatusCode::BAD_REQUEST);

        let fresh = call(
            &router,
            "PATCH",
            &path,
            wire,
            &[("if-match", "\"0\"")],
            Some(&json!({"balanceE8": I64_MAX})),
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(fresh.etag().as_deref(), Some("\"1\""), "{wire:?}");
        assert_eq!(fresh.json()["version"], Json::String("1".to_owned()));
        assert_eq!(fresh.json()["balanceE8"], Json::String(I64_MAX.to_owned()));

        // The version it was just bumped away from is stale, and changes nothing.
        call(
            &router,
            "PATCH",
            &path,
            wire,
            &[("if-match", "\"0\"")],
            Some(&json!({"balanceE8": I64_MIN})),
        )
        .await
        .expect(StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            db_i64(pool, "SELECT balance_e8 FROM bi_ledgers WHERE id = $1", id).await,
            i64::MAX
        );

        call(
            &router,
            "DELETE",
            &path,
            wire,
            &[("if-match", "\"0\"")],
            None,
        )
        .await
        .expect(StatusCode::PRECONDITION_FAILED);
        call(
            &router,
            "DELETE",
            &path,
            wire,
            &[("if-match", "\"1\"")],
            None,
        )
        .await
        .expect(StatusCode::OK);
    }
}

#[tokio::test]
async fn a_version_above_2_pow_53_is_an_exact_etag_and_an_exact_precondition() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    for (id, version) in [(10_i64, 9_007_199_254_740_993_i64), (11, i64::MAX - 1)] {
        cratestack::sqlx::query(
            "INSERT INTO bi_ledgers (id, balance_e8, version) VALUES ($1, 1, $2)",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await
        .expect("seed ledger");
    }

    for wire in Wire::BOTH {
        let path = "/bi_ledgers/10";
        let current = db_i64(pool, "SELECT version FROM bi_ledgers WHERE id = $1", 10).await;
        let got = call(&router, "GET", path, wire, &[], None)
            .await
            .expect(StatusCode::OK);
        assert_eq!(
            got.etag(),
            Some(format!("\"{current}\"")),
            "{wire:?}: ETag is the exact version"
        );
        assert_eq!(text(&got.json()["version"]), current.to_string());
        if wire == Wire::Cbor {
            assert!(has_cbor_text(&got.bytes, &current.to_string()));
        }

        // No If-Match at all is a 412 too (the contract `banking_versioning.rs`
        // pins for `Int`), on PATCH and on DELETE, and changes nothing.
        call(
            &router,
            "PATCH",
            path,
            wire,
            &[],
            Some(&json!({"balanceE8": "2"})),
        )
        .await
        .expect(StatusCode::PRECONDITION_FAILED);
        call(&router, "DELETE", path, wire, &[], None)
            .await
            .expect(StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            db_i64(pool, "SELECT version FROM bi_ledgers WHERE id = $1", 10).await,
            current,
            "{wire:?}: a request without If-Match must not change the row"
        );

        // One below the real version: a number routed through an f64 would
        // equate the two at 2^53 + 1, so a loose If-Match would pass here.
        let off_by_one = (current - 1).to_string();
        call(
            &router,
            "PATCH",
            path,
            wire,
            &[("if-match", &format!("\"{off_by_one}\""))],
            Some(&json!({"balanceE8": "2"})),
        )
        .await
        .expect(StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            db_i64(pool, "SELECT version FROM bi_ledgers WHERE id = $1", 10).await,
            current,
            "{wire:?}: a stale If-Match must not bump the version"
        );

        let bumped = call(
            &router,
            "PATCH",
            path,
            wire,
            &[("if-match", &format!("\"{current}\""))],
            Some(&json!({"balanceE8": "2"})),
        )
        .await
        .expect(StatusCode::OK);
        let next = (current + 1).to_string();
        assert_eq!(bumped.etag(), Some(format!("\"{next}\"")), "{wire:?}");
        assert_eq!(text(&bumped.json()["version"]), next);
        assert_eq!(
            db_i64(pool, "SELECT version FROM bi_ledgers WHERE id = $1", 10).await,
            current + 1
        );
    }
    assert!(
        db_i64(pool, "SELECT version FROM bi_ledgers WHERE id = $1", 10).await
            > 9_007_199_254_740_993,
        "both wires bumped the version"
    );

    // Up to the top of the range: i64::MAX - 1 -> i64::MAX.
    let top = call(
        &router,
        "PATCH",
        "/bi_ledgers/11",
        Wire::Json,
        &[("if-match", "\"9223372036854775806\"")],
        Some(&json!({"balanceE8": "9"})),
    )
    .await
    .expect(StatusCode::OK);
    assert_eq!(top.etag().as_deref(), Some("\"9223372036854775807\""));
    assert_eq!(text(&top.json()["version"]), I64_MAX);
    assert_eq!(
        db_i64(pool, "SELECT version FROM bi_ledgers WHERE id = $1", 11).await,
        i64::MAX
    );
}

// ---------------------------------------------------------------------------
// @range ... @db_enforce on BigInt: the API answers 422, the database a CHECK
// ---------------------------------------------------------------------------

#[tokio::test]
async fn range_on_a_bigint_is_enforced_by_the_api_with_a_422_naming_the_field() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    let refused = [
        ("smallE8", "-1", "minimum"),
        ("smallE8", "1000001", "maximum"),
        ("smallE8", ABOVE_2_53, "maximum"),
        ("smallE8", I64_MAX, "maximum"),
        ("smallE8", I64_MIN, "minimum"),
        // One outside each end of the widest range the grammar allows.
        ("wideE8", I64_MIN, "minimum"),
        ("wideE8", I64_MAX, "maximum"),
    ];
    for wire in Wire::BOTH {
        for (field, value, bound) in refused {
            let mut body = json!({"id": 1, "smallE8": "0", "wideE8": "0"});
            body[field] = Json::String(value.to_owned());
            let reply = call(&router, "POST", "/bi_limits", wire, &[], Some(&body))
                .await
                .expect(StatusCode::UNPROCESSABLE_ENTITY);
            let message = reply.json()["message"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            assert!(
                message.contains(field) && message.contains(bound),
                "{wire:?} {field}={value}: the 422 must name the field and the bound, got `{message}`"
            );
        }
    }
    let rows: i64 = cratestack::sqlx::query_scalar("SELECT count(*) FROM bi_limits")
        .fetch_one(pool)
        .await
        .expect("count");
    assert_eq!(rows, 0, "a refused create must write nothing");
}

#[tokio::test]
async fn range_on_a_bigint_accepts_both_inclusive_bounds_exactly() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    let accepted = [
        (1_i64, "0", "-9223372036854775807"),
        (2, "1000000", "9223372036854775806"),
        (3, "999999", ABOVE_2_53),
    ];
    for (wire, offset) in [(Wire::Json, 0_i64), (Wire::Cbor, 10)] {
        for (id, small, wide) in accepted {
            let created = call(
                &router,
                "POST",
                "/bi_limits",
                wire,
                &[],
                Some(&json!({"id": id + offset, "smallE8": small, "wideE8": wide})),
            )
            .await
            .expect(StatusCode::CREATED);
            assert_eq!(text(&created.json()["wideE8"]), wide);
            assert_eq!(
                db_i64(
                    pool,
                    "SELECT wide_e8 FROM bi_limits WHERE id = $1",
                    id + offset
                )
                .await,
                wide.parse::<i64>().expect("fits")
            );
        }
    }

    // An update is validated too, and a refused one changes nothing.
    for wire in Wire::BOTH {
        for (field, value) in [
            ("smallE8", "1000001"),
            ("smallE8", "-1"),
            ("wideE8", I64_MAX),
        ] {
            call(
                &router,
                "PATCH",
                "/bi_limits/1",
                wire,
                &[],
                Some(&json!({ field: value })),
            )
            .await
            .expect(StatusCode::UNPROCESSABLE_ENTITY);
        }
        let ok = call(
            &router,
            "PATCH",
            "/bi_limits/1",
            wire,
            &[],
            Some(&json!({"smallE8": "1000000", "wideE8": "9223372036854775806"})),
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(text(&ok.json()["wideE8"]), "9223372036854775806");
    }
}

#[tokio::test]
async fn range_on_a_bigint_is_also_a_check_constraint_in_the_database() {
    fixture!(test_pg, _router);
    let pool = &test_pg.pool;

    let checks: i64 = cratestack::sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint \
         WHERE conrelid = 'bi_limits'::regclass AND contype = 'c'",
    )
    .fetch_one(pool)
    .await
    .expect("count checks");
    assert_eq!(
        checks, 2,
        "@db_enforce on two BigInt columns must emit two CHECKs"
    );

    let insert = |id: i64, small: i64, wide: i64| async move {
        cratestack::sqlx::query("INSERT INTO bi_limits (id, small_e8, wide_e8) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(small)
            .bind(wide)
            .execute(pool)
            .await
    };
    let violations = [
        (1, -1, 0, "small_e8"),
        (2, 1_000_001, 0, "small_e8"),
        (3, 0, i64::MIN, "wide_e8"),
        (4, 0, i64::MAX, "wide_e8"),
    ];
    for (id, small, wide, column) in violations {
        let error = insert(id, small, wide)
            .await
            .expect_err("the database must refuse an out-of-range BigInt");
        let database_error = error.as_database_error().expect("a database error");
        assert_eq!(
            database_error.code().as_deref(),
            Some("23514"),
            "({small}, {wide}): expected check_violation, got {database_error}"
        );
        assert!(
            database_error
                .constraint()
                .is_some_and(|name| name.contains(column)),
            "({small}, {wide}): the violated constraint must be on {column}, got {:?}",
            database_error.constraint()
        );
    }
    insert(5, 0, i64::MIN + 1)
        .await
        .expect("the lower bound is inclusive");
    insert(6, 1_000_000, i64::MAX - 1)
        .await
        .expect("the upper bound is inclusive");
}

// ---------------------------------------------------------------------------
// a BigInt foreign key and a relation include
// ---------------------------------------------------------------------------

async fn seed_teams(router: &Router) {
    for (id, name) in [(I64_MAX, "max"), (ABOVE_2_53, "mid")] {
        call(
            router,
            "POST",
            "/bi_teams",
            Wire::Json,
            &[],
            Some(&json!({"id": id, "name": name})),
        )
        .await
        .expect(StatusCode::CREATED);
    }
    for (id, team, score) in [
        (I64_MIN, I64_MAX, I64_MIN),
        (TWO_53, I64_MAX, I64_MAX),
        ("1", ABOVE_2_53, TWO_53),
    ] {
        call(
            router,
            "POST",
            "/bi_players",
            Wire::Cbor,
            &[],
            Some(&json!({"id": id, "teamId": team, "scoreE8": score})),
        )
        .await
        .expect(StatusCode::CREATED);
    }
}

#[tokio::test]
async fn a_bigint_foreign_key_loads_its_parent_through_include() {
    fixture!(test_pg, router);
    seed_teams(&router).await;
    for wire in Wire::BOTH {
        let player = call(
            &router,
            "GET",
            &format!("/bi_players/{I64_MIN}?include=team"),
            wire,
            &[],
            None,
        )
        .await
        .expect(StatusCode::OK);
        let body = player.json();
        assert_eq!(text(&body["teamId"]), I64_MAX, "{wire:?}");
        assert_eq!(
            text(&body["team"]["id"]),
            I64_MAX,
            "{wire:?}: the included parent"
        );
        assert_eq!(body["team"]["name"], "max");
        assert_eq!(text(&body["scoreE8"]), I64_MIN);
        if wire == Wire::Cbor {
            assert!(has_cbor_text(&player.bytes, I64_MAX));
        }
    }
}

#[tokio::test]
async fn a_to_many_include_groups_children_by_their_bigint_key() {
    fixture!(test_pg, router);
    seed_teams(&router).await;
    for wire in Wire::BOTH {
        let one = call(
            &router,
            "GET",
            &format!("/bi_teams/{I64_MAX}?include=players"),
            wire,
            &[],
            None,
        )
        .await
        .expect(StatusCode::OK);
        let mut children: Vec<String> = one.json()["players"]
            .as_array()
            .expect("players array")
            .iter()
            .map(|p| {
                assert_eq!(text(&p["teamId"]), I64_MAX);
                text(&p["id"]).to_owned()
            })
            .collect();
        children.sort();
        assert_eq!(
            children,
            vec![I64_MIN.to_owned(), TWO_53.to_owned()],
            "{wire:?}"
        );

        // The batched path: one query for every team's children, grouped by key.
        let many = call(
            &router,
            "GET",
            "/bi_teams?include=players&sort=id",
            wire,
            &[],
            None,
        )
        .await
        .expect(StatusCode::OK);
        let teams = many.json();
        let teams = teams.as_array().expect("array");
        assert_eq!(teams.len(), 2);
        let counts: Vec<(String, usize)> = teams
            .iter()
            .map(|team| {
                (
                    text(&team["id"]).to_owned(),
                    team["players"].as_array().expect("players").len(),
                )
            })
            .collect();
        assert_eq!(
            counts,
            vec![(ABOVE_2_53.to_owned(), 1), (I64_MAX.to_owned(), 2)],
            "{wire:?}: every child must land under its own BigInt parent key"
        );
    }
}

#[tokio::test]
async fn a_bigint_foreign_key_filters_and_is_enforced_by_the_database() {
    fixture!(test_pg, router);
    seed_teams(&router).await;
    for wire in Wire::BOTH {
        let filtered = call(
            &router,
            "GET",
            &format!("/bi_players?teamId={I64_MAX}&fields=id&sort=id"),
            wire,
            &[],
            None,
        )
        .await
        .expect(StatusCode::OK);
        let rows: Vec<String> = filtered
            .json()
            .as_array()
            .expect("array")
            .iter()
            .map(|row| text(&row["id"]).to_owned())
            .collect();
        assert_eq!(
            rows,
            vec![I64_MIN.to_owned(), TWO_53.to_owned()],
            "{wire:?}"
        );
    }

    // No team 5: the foreign key must refuse it rather than store a dangling id.
    let dangling = call(
        &router,
        "POST",
        "/bi_players",
        Wire::Json,
        &[],
        Some(&json!({"id": "99", "teamId": "5", "scoreE8": "1"})),
    )
    .await;
    // The framework does not map SQLSTATE 23503 (foreign_key_violation) to a 4xx
    // today, for `Int` keys either, so only "not accepted" is asserted here.
    assert!(
        !dangling.status.is_success(),
        "a foreign key to a missing BigInt parent must be refused, got {}",
        dangling.status
    );
    let foreign_keys: i64 = cratestack::sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint \
         WHERE conrelid = 'bi_players'::regclass AND contype = 'f'",
    )
    .fetch_one(&test_pg.pool)
    .await
    .expect("count foreign keys");
    assert_eq!(
        foreign_keys, 1,
        "the BigInt foreign key must exist in the emitted DDL"
    );
    let stored: i64 =
        cratestack::sqlx::query_scalar("SELECT count(*) FROM bi_players WHERE id = 99")
            .fetch_one(&test_pg.pool)
            .await
            .expect("count");
    assert_eq!(stored, 0);
}

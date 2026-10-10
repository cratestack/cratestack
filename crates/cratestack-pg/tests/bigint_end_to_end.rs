//! ADR 0019 PR B (B11): a `BigInt` model served over REST, against a real
//! Postgres, on JSON and on CBOR, at `i64::MAX`, `i64::MIN` and `2^53 + 1`.
//!
//! Covers the `cratestack-pg` bullet of `docs/design/int-and-bigint.md`
//! section 7: create, get, list with `?fields=`, update, delete and the
//! `gt`/`lt`/`in` filters; the `<Model>Where` of a `FindMany` procedure; BigInt
//! procedure arguments and returns; the typed delegate on a `BigInt` primary
//! key. `bigint_end_to_end_models.rs` carries `@version`, `@range`/`@db_enforce`
//! and the relation include; `bigint_end_to_end_rpc.rs` the RPC transport.
//!
//! Every table comes from the real migration emitter (`bigint_support::reset`),
//! never a hand-written `CREATE TABLE`: a `BigInt` emitted as `TEXT` fails here
//! on the first bind, which is the silent-fallback class ADR 0019 risk 3 names.
//!
//! Run with `CRATESTACK_REQUIRE_DB=1` (and `DOCKER_HOST` on rootless Docker): a
//! skipped PG binary prints `ok` in `0.00s`.

use cratestack::axum::Router;
use cratestack::axum::http::StatusCode;
use cratestack::serde_json::{Value as Json, json};
use cratestack::sqlx::PgPool;
use cratestack::{BigInt, CodecSet, CratestackContext, Value, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

include_server_schema!("tests/fixtures/bigint_end_to_end.cstack", db = Postgres);

mod bigint_end_to_end_support;
mod bigint_support;
mod support;

use bigint_end_to_end_support::Procedures;
use bigint_support::{
    ABOVE_2_53, BOUNDARY, I64_MAX, I64_MIN, Reply, Wire, call, call_raw, cbor_map, cbor_text,
    column_type, db_i64, has_cbor_integer, has_cbor_text, operator_auth, reset,
};
use cratestack_schema::{BiAccount, CreateBiAccountInput, UpdateBiAccountInput, bi_account};
use support::pg;

const SCHEMA: &str = include_str!("fixtures/bigint_end_to_end.cstack");
const TABLES: &str = "bi_players, bi_teams, bi_limits, bi_ledgers, bi_accounts";
const TWO_53: &str = "9007199254740992";
const TWO_53_PLUS_2: &str = "9007199254740994";

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

/// One shared preamble: serialise, start (or skip) the database, apply the
/// emitted DDL, build the router.
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

fn ctx() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

/// The rows the list tests seed: `id == amountE8`, with and without a fee.
const ROWS: [(&str, Option<&str>); 5] = [
    (I64_MIN, None),
    (TWO_53, Some(TWO_53)),
    (ABOVE_2_53, Some(ABOVE_2_53)),
    (TWO_53_PLUS_2, None),
    (I64_MAX, Some(I64_MAX)),
];

async fn seed(pool: &PgPool) {
    for (text, fee) in ROWS {
        let n: i64 = text.parse().expect("seed value fits i64");
        let fee: Option<i64> = fee.map(|f| f.parse().expect("fee fits i64"));
        cratestack::sqlx::query(
            "INSERT INTO bi_accounts (id, amount_e8, fee_e8, label) VALUES ($1, $1, $2, 'seed')",
        )
        .bind(n)
        .bind(fee)
        .execute(pool)
        .await
        .expect("seed row");
    }
}

fn ids(reply: &Reply) -> Vec<String> {
    reply
        .json()
        .as_array()
        .unwrap_or_else(|| panic!("a list answer is an array: {}", reply.json()))
        .iter()
        .map(|row| {
            row["id"]
                .as_str()
                .unwrap_or_else(|| panic!("a BigInt id is a string, got {row}"))
                .to_owned()
        })
        .collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

// ---------------------------------------------------------------------------
// create / get / update / delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_bigint_columns_are_bigint_in_postgres() {
    fixture!(test_pg, _router);
    for (table, column) in [
        ("bi_accounts", "id"),
        ("bi_accounts", "amount_e8"),
        ("bi_accounts", "fee_e8"),
        ("bi_ledgers", "version"),
        ("bi_players", "team_id"),
    ] {
        assert_eq!(
            column_type(&test_pg.pool, table, column).await,
            "bigint",
            "{table}.{column}: a BigInt column that is not BIGINT means the emitter fell back to TEXT"
        );
    }
}

#[tokio::test]
async fn create_get_update_delete_round_trip_the_boundary_values_on_both_codecs() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    for wire in Wire::BOTH {
        for (text, n) in BOUNDARY {
            let path = format!("/bi_accounts/{text}");

            // create: strings in, strings out, the exact i64 in the database.
            let created = call(
                &router,
                "POST",
                "/bi_accounts",
                wire,
                &[],
                Some(&json!({"id": text, "amountE8": text, "feeE8": text, "label": "x"})),
            )
            .await
            .expect(StatusCode::CREATED);
            let body = created.json();
            for key in ["id", "amountE8", "feeE8"] {
                assert_eq!(
                    body[key],
                    Json::String(text.to_owned()),
                    "{wire:?} create {key}"
                );
            }
            if wire == Wire::Cbor {
                assert!(
                    has_cbor_text(&created.bytes, text),
                    "{text}: not a CBOR text string"
                );
                assert!(
                    !has_cbor_integer(&created.bytes, n),
                    "{text}: a CBOR integer on the wire"
                );
            }
            for (column, sql) in [
                ("id", "SELECT id FROM bi_accounts WHERE id = $1"),
                (
                    "amount_e8",
                    "SELECT amount_e8 FROM bi_accounts WHERE id = $1",
                ),
                ("fee_e8", "SELECT fee_e8 FROM bi_accounts WHERE id = $1"),
            ] {
                assert_eq!(db_i64(pool, sql, n).await, n, "{wire:?} stored {column}");
            }

            // get
            let got = call(&router, "GET", &path, wire, &[], None)
                .await
                .expect(StatusCode::OK);
            assert_eq!(got.json()["amountE8"], Json::String(text.to_owned()));
            assert_eq!(got.json()["id"], Json::String(text.to_owned()));
            if wire == Wire::Cbor {
                assert!(has_cbor_text(&got.bytes, text));
                assert!(!has_cbor_integer(&got.bytes, n));
            }

            // update: another boundary value, and an explicit null clears fee.
            let other = BOUNDARY[(BOUNDARY.iter().position(|b| b.0 == text).unwrap() + 1) % 3];
            let updated = call(
                &router,
                "PATCH",
                &path,
                wire,
                &[],
                Some(&json!({"amountE8": other.0, "feeE8": null})),
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(updated.json()["amountE8"], Json::String(other.0.to_owned()));
            assert_eq!(
                updated.json()["feeE8"],
                Json::Null,
                "{wire:?}: null clears an optional BigInt"
            );
            assert_eq!(
                db_i64(pool, "SELECT amount_e8 FROM bi_accounts WHERE id = $1", n).await,
                other.1
            );
            let fee: Option<i64> =
                cratestack::sqlx::query_scalar("SELECT fee_e8 FROM bi_accounts WHERE id = $1")
                    .bind(n)
                    .fetch_one(pool)
                    .await
                    .expect("fee column");
            assert_eq!(fee, None);

            // an update that touches only the label leaves the BigInts alone.
            let relabelled = call(
                &router,
                "PATCH",
                &path,
                wire,
                &[],
                Some(&json!({"label": "y"})),
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(
                relabelled.json()["amountE8"],
                Json::String(other.0.to_owned())
            );

            // delete returns the row, and it is gone.
            let deleted = call(&router, "DELETE", &path, wire, &[], None)
                .await
                .expect(StatusCode::OK);
            assert_eq!(deleted.json()["id"], Json::String(text.to_owned()));
            call(&router, "GET", &path, wire, &[], None)
                .await
                .expect(StatusCode::NOT_FOUND);
        }
    }
}

#[tokio::test]
async fn creating_the_same_bigint_key_twice_is_a_conflict() {
    fixture!(test_pg, router);
    let _ = &test_pg;
    let body = json!({"id": I64_MAX, "amountE8": "1", "label": "x"});
    call(
        &router,
        "POST",
        "/bi_accounts",
        Wire::Json,
        &[],
        Some(&body),
    )
    .await
    .expect(StatusCode::CREATED);
    call(
        &router,
        "POST",
        "/bi_accounts",
        Wire::Json,
        &[],
        Some(&body),
    )
    .await
    .expect(StatusCode::CONFLICT);
}

// ---------------------------------------------------------------------------
// list, ?fields=, sort, filters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_with_fields_projects_bigint_as_strings_on_both_codecs() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        let reply = call(
            &router,
            "GET",
            "/bi_accounts?fields=id,amountE8,feeE8&sort=id",
            wire,
            &[],
            None,
        )
        .await
        .expect(StatusCode::OK);
        let rows = reply.json();
        let rows = rows.as_array().expect("array");
        assert_eq!(rows.len(), 5);
        for (row, (text, fee)) in rows.iter().zip(ROWS) {
            let object = row.as_object().expect("object");
            assert_eq!(
                object.len(),
                3,
                "{wire:?}: ?fields= must project exactly the asked fields"
            );
            assert_eq!(row["id"], Json::String(text.to_owned()), "{wire:?}");
            assert_eq!(row["amountE8"], Json::String(text.to_owned()), "{wire:?}");
            assert_eq!(
                row["feeE8"],
                fee.map_or(Json::Null, |f| Json::String(f.to_owned())),
                "{wire:?} fee of {text}"
            );
        }
        if wire == Wire::Cbor {
            for (text, n) in BOUNDARY {
                assert!(
                    has_cbor_text(&reply.bytes, text),
                    "{text}: projected leaf is not a text string"
                );
                assert!(
                    !has_cbor_integer(&reply.bytes, n),
                    "{text}: projected as a CBOR integer"
                );
            }
        }
    }
}

#[tokio::test]
async fn a_projection_of_only_the_key_is_still_a_string() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        let reply = call(
            &router,
            "GET",
            "/bi_accounts?fields=id&sort=-id",
            wire,
            &[],
            None,
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(
            ids(&reply),
            strings(&[I64_MAX, TWO_53_PLUS_2, ABOVE_2_53, TWO_53, I64_MIN]),
            "{wire:?}: sort by a BigInt column must be numeric, not lexical"
        );
    }
}

#[tokio::test]
async fn get_with_fields_projects_a_bigint_key() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        let reply = call(
            &router,
            "GET",
            &format!("/bi_accounts/{I64_MAX}?fields=id,amountE8"),
            wire,
            &[],
            None,
        )
        .await
        .expect(StatusCode::OK);
        let body = reply.json();
        assert_eq!(body.as_object().expect("object").len(), 2);
        assert_eq!(body["id"], Json::String(I64_MAX.to_owned()));
        assert_eq!(body["amountE8"], Json::String(I64_MAX.to_owned()));
    }
}

#[tokio::test]
async fn gt_gte_lt_lte_eq_ne_and_in_filters_compare_numerically_at_the_boundaries() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    let cases: Vec<(String, Vec<String>)> = vec![
        (
            format!("amountE8__gt={ABOVE_2_53}"),
            strings(&[TWO_53_PLUS_2, I64_MAX]),
        ),
        (
            format!("amountE8__gte={ABOVE_2_53}"),
            strings(&[ABOVE_2_53, TWO_53_PLUS_2, I64_MAX]),
        ),
        (
            format!("amountE8__lt={ABOVE_2_53}"),
            strings(&[I64_MIN, TWO_53]),
        ),
        (
            format!("amountE8__lte={ABOVE_2_53}"),
            strings(&[I64_MIN, TWO_53, ABOVE_2_53]),
        ),
        (format!("amountE8={ABOVE_2_53}"), strings(&[ABOVE_2_53])),
        (format!("amountE8__eq={ABOVE_2_53}"), strings(&[ABOVE_2_53])),
        (
            format!("amountE8__ne={ABOVE_2_53}"),
            strings(&[I64_MIN, TWO_53, TWO_53_PLUS_2, I64_MAX]),
        ),
        (
            format!("amountE8__in={I64_MAX},{I64_MIN}"),
            strings(&[I64_MIN, I64_MAX]),
        ),
        (format!("amountE8__in={ABOVE_2_53}"), strings(&[ABOVE_2_53])),
        (
            "amountE8__gt=9223372036854775806".to_owned(),
            strings(&[I64_MAX]),
        ),
        (
            "amountE8__lt=-9223372036854775807".to_owned(),
            strings(&[I64_MIN]),
        ),
        // The primary key is a BigInt too.
        (
            format!("id__gt={ABOVE_2_53}"),
            strings(&[TWO_53_PLUS_2, I64_MAX]),
        ),
        // The optional BigInt.
        (
            "feeE8__isNull=true".to_owned(),
            strings(&[I64_MIN, TWO_53_PLUS_2]),
        ),
        (format!("feeE8={I64_MAX}"), strings(&[I64_MAX])),
        // `where=` re-enters the same value parser. Commas split its AND terms,
        // so a multi-member `in` cannot appear inside it: one member, and a group.
        (
            format!("where=amountE8__gt={ABOVE_2_53}"),
            strings(&[TWO_53_PLUS_2, I64_MAX]),
        ),
        (
            format!("where=amountE8__in={ABOVE_2_53}"),
            strings(&[ABOVE_2_53]),
        ),
        (
            format!("where=(amountE8__in={I64_MAX}%7CamountE8={I64_MIN})"),
            strings(&[I64_MIN, I64_MAX]),
        ),
        (
            format!("where=amountE8__gte={TWO_53},amountE8__lt={TWO_53_PLUS_2}"),
            strings(&[TWO_53, ABOVE_2_53]),
        ),
        (
            format!("where=(amountE8__gt={ABOVE_2_53}%7CamountE8__lt=-1)"),
            strings(&[I64_MIN, TWO_53_PLUS_2, I64_MAX]),
        ),
        (
            format!("where=not(amountE8={ABOVE_2_53})"),
            strings(&[I64_MIN, TWO_53, TWO_53_PLUS_2, I64_MAX]),
        ),
    ];
    for wire in Wire::BOTH {
        for (query, expected) in &cases {
            let reply = call(
                &router,
                "GET",
                &format!("/bi_accounts?{query}&sort=id"),
                wire,
                &[],
                None,
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(&ids(&reply), expected, "{wire:?} ?{query}");
        }
    }
}

#[tokio::test]
async fn a_filter_value_that_is_not_a_canonical_bigint_is_a_400() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for bad in [
        "9223372036854775808",
        "-9223372036854775809",
        "abc",
        "007",
        "%2B5",
        "-0",
        "1.5",
        "1e3",
        "0x10",
        "",
        "%201",
    ] {
        for operator in ["", "__gt", "__lt", "__ne", "__in"] {
            // An `in` list is split on commas and each member trimmed, exactly as
            // for `Int`, so surrounding whitespace is not part of the value there.
            if operator == "__in" && bad == "%201" {
                continue;
            }
            let reply = call(
                &router,
                "GET",
                &format!("/bi_accounts?amountE8{operator}={bad}"),
                Wire::Json,
                &[],
                None,
            )
            .await;
            assert_eq!(
                reply.status,
                StatusCode::BAD_REQUEST,
                "?amountE8{operator}={bad:?} must be refused, got {} {}",
                reply.status,
                String::from_utf8_lossy(&reply.bytes)
            );
        }
    }
    // `where=` is the same parser.
    for bad in ["9223372036854775808", "007", "abc", "-0"] {
        for query in [
            format!("where=amountE8__gt={bad}"),
            format!("where=amountE8={bad}"),
            format!("where=(amountE8__in={bad}%7CamountE8=1)"),
        ] {
            let reply = call(
                &router,
                "GET",
                &format!("/bi_accounts?{query}"),
                Wire::Json,
                &[],
                None,
            )
            .await;
            assert_eq!(
                reply.status,
                StatusCode::BAD_REQUEST,
                "?{query} must be refused"
            );
        }
    }
    // One bad member poisons a whole `in` list.
    let reply = call(
        &router,
        "GET",
        &format!("/bi_accounts?amountE8__in={I64_MAX},007"),
        Wire::Json,
        &[],
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// refusals on the request body
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_number_at_a_bigint_key_is_a_400_and_writes_nothing() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    let be_i64_max = [0x48, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];

    // JSON: a number, exact or not, at the key, at an optional key, at the PK.
    for raw in [
        "5",
        "0",
        "9007199254740993",
        "9223372036854775807",
        "5.0",
        "true",
    ] {
        for key in ["id", "amountE8", "feeE8"] {
            let mut entries = vec![
                ("id", format!(r#""{ABOVE_2_53}""#)),
                ("amountE8", r#""1""#.to_owned()),
                ("label", r#""x""#.to_owned()),
            ];
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = raw.to_owned(),
                None => entries.push((key, raw.to_owned())),
            }
            let body = entries
                .iter()
                .map(|(k, v)| format!(r#""{k}":{v}"#))
                .collect::<Vec<_>>()
                .join(",");
            let reply = call_raw(
                &router,
                "POST",
                "/bi_accounts",
                Wire::Json,
                &[],
                Some(format!("{{{body}}}").into_bytes()),
            )
            .await;
            assert_eq!(reply.status, StatusCode::BAD_REQUEST, "JSON {raw} at {key}");
            assert_eq!(reply.json()["code"], "CODEC_ERROR", "JSON {raw} at {key}");
        }
    }

    // CBOR: a major type 0/1 integer and a tag 2/3 bignum, at every key.
    let refused_items: [(&str, Vec<u8>); 5] = [
        ("uint", vec![0x05]),
        ("nint", vec![0x20]),
        (
            "uint64",
            vec![0x1b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        ),
        ("tag 2", [&[0xc2][..], &be_i64_max].concat()),
        ("tag 3", [&[0xc3][..], &be_i64_max].concat()),
    ];
    for (label, item) in refused_items {
        for key in ["id", "amountE8", "feeE8"] {
            let mut entries = vec![
                ("id", cbor_text(ABOVE_2_53)),
                ("amountE8", cbor_text("1")),
                ("label", cbor_text("x")),
            ];
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = item.clone(),
                None => entries.push((key, item.clone())),
            }
            let reply = call_raw(
                &router,
                "POST",
                "/bi_accounts",
                Wire::Cbor,
                &[],
                Some(cbor_map(&entries)),
            )
            .await;
            assert_eq!(
                reply.status,
                StatusCode::BAD_REQUEST,
                "CBOR {label} at {key}"
            );
            assert_eq!(reply.json()["code"], "CODEC_ERROR", "CBOR {label} at {key}");
        }
    }

    // Strings that are not the canonical form.
    for bad in [
        "+5",
        "007",
        "-0",
        " 1",
        "1 ",
        "",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        for wire in Wire::BOTH {
            let reply = call(
                &router,
                "POST",
                "/bi_accounts",
                wire,
                &[],
                Some(&json!({"id": "1", "amountE8": bad, "label": "x"})),
            )
            .await;
            assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{wire:?} {bad:?}");
        }
    }

    let count: i64 = cratestack::sqlx::query_scalar("SELECT count(*) FROM bi_accounts")
        .fetch_one(pool)
        .await
        .expect("count");
    assert_eq!(count, 0, "a refused body must not write a row");
}

#[tokio::test]
async fn a_number_in_a_patch_is_a_400_and_leaves_the_row_alone() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        let reply = call(
            &router,
            "PATCH",
            &format!("/bi_accounts/{I64_MAX}"),
            wire,
            &[],
            Some(&json!({"amountE8": 5})),
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{wire:?}");
    }
    assert_eq!(
        db_i64(
            &test_pg.pool,
            "SELECT amount_e8 FROM bi_accounts WHERE id = $1",
            i64::MAX
        )
        .await,
        i64::MAX
    );
}

#[tokio::test]
async fn a_path_key_that_is_not_a_canonical_bigint_is_refused() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for bad in ["007", "abc", "9223372036854775808", "-0", "%2B5", "1.5"] {
        for method in ["GET", "DELETE"] {
            let reply = call(
                &router,
                method,
                &format!("/bi_accounts/{bad}"),
                Wire::Json,
                &[],
                None,
            )
            .await;
            assert!(
                reply.status.is_client_error(),
                "{method} /bi_accounts/{bad} must be refused as malformed, got {}",
                reply.status
            );
        }
    }
    let count: i64 = cratestack::sqlx::query_scalar("SELECT count(*) FROM bi_accounts")
        .fetch_one(&test_pg.pool)
        .await
        .expect("count");
    assert_eq!(count, 5, "a refused path key must not delete anything");
}

// ---------------------------------------------------------------------------
// procedures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_procedure_takes_and_returns_bigint_scalars_options_and_lists() {
    fixture!(test_pg, router);
    let _ = &test_pg;
    for wire in Wire::BOTH {
        for (text, n) in BOUNDARY {
            let reply = call(
                &router,
                "POST",
                "/$procs/biEcho",
                wire,
                &[],
                Some(&json!({"args": {
                    "amountE8": text,
                    "feeE8": text,
                    "tagsE8": [I64_MIN, text, I64_MAX],
                }})),
            )
            .await
            .expect(StatusCode::OK);
            let body = reply.json();
            assert_eq!(body["amountE8"], Json::String(text.to_owned()), "{wire:?}");
            assert_eq!(body["feeE8"], Json::String(text.to_owned()));
            assert_eq!(body["tagsE8"], json!([I64_MIN, text, I64_MAX]));
            if wire == Wire::Cbor {
                assert!(has_cbor_text(&reply.bytes, text));
                assert!(!has_cbor_integer(&reply.bytes, n));
            }

            let absent = call(
                &router,
                "POST",
                "/$procs/biEcho",
                wire,
                &[],
                Some(&json!({"args": {"amountE8": text, "feeE8": null, "tagsE8": []}})),
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(absent.json()["feeE8"], Json::Null);
            assert_eq!(absent.json()["tagsE8"], json!([]));
        }
    }
}

#[tokio::test]
async fn a_bare_bigint_argument_and_a_bare_bigint_return_are_strings() {
    fixture!(test_pg, router);
    let _ = &test_pg;
    for wire in Wire::BOTH {
        let reply = call(
            &router,
            "POST",
            "/$procs/biNext",
            wire,
            &[],
            Some(&json!({"valueE8": "9007199254740992"})),
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(
            reply.json(),
            Json::String(ABOVE_2_53.to_owned()),
            "{wire:?}"
        );
        if wire == Wire::Cbor {
            assert_eq!(
                reply.bytes,
                cbor_text(ABOVE_2_53),
                "a bare BigInt return is a lone CBOR text string"
            );
        }

        let top = call(
            &router,
            "POST",
            "/$procs/biNext",
            wire,
            &[],
            Some(&json!({"valueE8": "9223372036854775806"})),
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(top.json(), Json::String(I64_MAX.to_owned()));

        // The handler's own checked_add refuses to wrap past i64::MAX.
        call(
            &router,
            "POST",
            "/$procs/biNext",
            wire,
            &[],
            Some(&json!({"valueE8": I64_MAX})),
        )
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);

        // A number as the argument is refused before the handler runs.
        call(
            &router,
            "POST",
            "/$procs/biNext",
            wire,
            &[],
            Some(&json!({"valueE8": 5})),
        )
        .await
        .expect(StatusCode::BAD_REQUEST);
        call(
            &router,
            "POST",
            "/$procs/biEcho",
            wire,
            &[],
            Some(&json!({"args": {"amountE8": "1", "feeE8": null, "tagsE8": ["1", 2]}})),
        )
        .await
        .expect(StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn find_many_where_on_a_bigint_field_really_filters() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    let find = |wire: Wire, query: Json| {
        let router = router.clone();
        async move {
            call(
                &router,
                "POST",
                "/$procs/biFindAccounts",
                wire,
                &[],
                Some(&json!({ "query": query })),
            )
            .await
            .expect(StatusCode::OK)
        }
    };
    for wire in Wire::BOTH {
        // Risk 3: a `Where` without the field decodes fine, drops the key and
        // answers every row. Five rows come back only if the filter is ignored.
        let unfiltered = ids(&find(wire, json!({})).await);
        assert_eq!(unfiltered.len(), 5);

        let cases = [
            (
                json!({"where": {"amountE8": {"gt": ABOVE_2_53}}}),
                strings(&[TWO_53_PLUS_2, I64_MAX]),
            ),
            (
                json!({"where": {"amountE8": {"lt": ABOVE_2_53}}}),
                strings(&[I64_MIN, TWO_53]),
            ),
            (
                json!({"where": {"amountE8": {"in": [I64_MAX, I64_MIN]}}}),
                strings(&[I64_MIN, I64_MAX]),
            ),
            (
                json!({"where": {"amountE8": {"eq": ABOVE_2_53}}}),
                strings(&[ABOVE_2_53]),
            ),
            (
                json!({"where": {"amountE8": {"gte": TWO_53, "lte": ABOVE_2_53}}}),
                strings(&[TWO_53, ABOVE_2_53]),
            ),
            (
                json!({"where": {"id": {"ne": I64_MAX}, "feeE8": {"isNull": true}}}),
                strings(&[I64_MIN, TWO_53_PLUS_2]),
            ),
        ];
        for (query, expected) in cases {
            assert_eq!(
                ids(&find(wire, query.clone()).await),
                expected,
                "{wire:?} {query}"
            );
        }

        let descending = ids(&find(
            wire,
            json!({"orderBy": [{"field": "amountE8", "direction": "desc"}]}),
        )
        .await);
        assert_eq!(
            descending,
            strings(&[I64_MAX, TWO_53_PLUS_2, ABOVE_2_53, TWO_53, I64_MIN])
        );
    }
}

#[tokio::test]
async fn a_number_in_a_find_many_where_is_a_400() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        for filter in [
            json!({"gt": 5}),
            json!({"in": ["1", 2]}),
            json!({"eq": 9_007_199_254_740_993_i64}),
        ] {
            call(
                &router,
                "POST",
                "/$procs/biFindAccounts",
                wire,
                &[],
                Some(&json!({"query": {"where": {"amountE8": filter}}})),
            )
            .await
            .expect(StatusCode::BAD_REQUEST);
        }
    }
}

// ---------------------------------------------------------------------------
// the typed delegate: a BigInt primary key through sqlx
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_typed_delegate_round_trips_a_bigint_primary_key() {
    fixture!(test_pg, _router);
    let cool = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let ctx = ctx();

    for (_, n) in BOUNDARY {
        let id = BigInt::new(n);
        let created = cool
            .bi_account()
            .create(CreateBiAccountInput {
                id,
                amountE8: id,
                feeE8: Some(id),
                label: "typed".to_owned(),
            })
            .run(&ctx)
            .await
            .expect("create with a BigInt key");
        assert_eq!(created.id, id);
        assert_eq!(created.amountE8, id);

        let found: BiAccount = cool
            .bi_account()
            .find_unique(id)
            .run(&ctx)
            .await
            .expect("find_unique binds the key")
            .expect("row exists");
        assert_eq!(found, created);
    }

    let updated = cool
        .bi_account()
        .update(BigInt::MAX)
        .set(UpdateBiAccountInput {
            amountE8: Some(BigInt::MIN),
            ..Default::default()
        })
        .run(&ctx)
        .await
        .expect("update by a BigInt key");
    assert_eq!(updated.amountE8, BigInt::MIN);

    let above = cool
        .bi_account()
        .find_many()
        .where_(bi_account::id().gt(BigInt::new(0)))
        .order_by(bi_account::id().asc())
        .run(&ctx)
        .await
        .expect("gt on a BigInt column");
    assert_eq!(
        above.iter().map(|r| r.id.get()).collect::<Vec<_>>(),
        vec![9_007_199_254_740_993, i64::MAX]
    );

    let members = cool
        .bi_account()
        .find_many()
        .where_(bi_account::id().in_([BigInt::MAX, BigInt::MIN]))
        .order_by(bi_account::id().asc())
        .run(&ctx)
        .await
        .expect("in on a BigInt column");
    assert_eq!(
        members.iter().map(|r| r.id.get()).collect::<Vec<_>>(),
        vec![i64::MIN, i64::MAX]
    );

    let batch = cool
        .bi_account()
        .batch_get(vec![BigInt::MAX, BigInt::new(7), BigInt::MIN])
        .run(&ctx)
        .await
        .expect("batch_get binds every key");
    assert_eq!((batch.summary.ok, batch.summary.err), (2, 1));

    let deleted = cool
        .bi_account()
        .delete(BigInt::MAX)
        .run(&ctx)
        .await
        .expect("delete by a BigInt key");
    assert_eq!(deleted.id, BigInt::MAX);
    assert!(
        cool.bi_account()
            .find_unique(BigInt::MAX)
            .run(&ctx)
            .await
            .expect("find after delete")
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// a `db = Postgres` procedure with a BigInt argument under `@isolation`
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_isolated_procedure_reads_and_writes_a_bigint_key_through_its_transaction() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    seed(pool).await;
    for wire in Wire::BOTH {
        // 2^53 + 1 is the first value an f64 cannot hold; +1 keeps it exact.
        let current = db_i64(
            pool,
            "SELECT amount_e8 FROM bi_accounts WHERE id = $1",
            9_007_199_254_740_993,
        )
        .await;
        let reply = call(
            &router,
            "POST",
            "/$procs/biDeposit",
            wire,
            &[],
            Some(&json!({"args": {"accountId": ABOVE_2_53, "deltaE8": "1"}})),
        )
        .await
        .expect(StatusCode::OK);
        let expected = (current + 1).to_string();
        assert_eq!(
            reply.json()["amountE8"],
            Json::String(expected.clone()),
            "{wire:?}"
        );
        assert_eq!(reply.json()["id"], Json::String(ABOVE_2_53.to_owned()));
        assert_eq!(
            db_i64(
                pool,
                "SELECT amount_e8 FROM bi_accounts WHERE id = $1",
                9_007_199_254_740_993
            )
            .await,
            current + 1,
            "{wire:?}: the write inside the transaction committed"
        );
        if wire == Wire::Cbor {
            assert!(has_cbor_text(&reply.bytes, &expected));
        }
    }

    // Refused before the transaction opens: nothing changes.
    let before = db_i64(
        pool,
        "SELECT amount_e8 FROM bi_accounts WHERE id = $1",
        i64::MAX,
    )
    .await;
    for wire in Wire::BOTH {
        for args in [
            json!({"accountId": I64_MAX, "deltaE8": 1}),
            json!({"accountId": 5, "deltaE8": "1"}),
            json!({"accountId": I64_MAX, "deltaE8": "+1"}),
            json!({"accountId": "007", "deltaE8": "1"}),
        ] {
            call(
                &router,
                "POST",
                "/$procs/biDeposit",
                wire,
                &[],
                Some(&json!({ "args": args })),
            )
            .await
            .expect(StatusCode::BAD_REQUEST);
        }
        // The body's own checked_add: i64::MAX + 1 is an error, not a wrap.
        call(
            &router,
            "POST",
            "/$procs/biDeposit",
            wire,
            &[],
            Some(&json!({"args": {"accountId": I64_MAX, "deltaE8": "1"}})),
        )
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
        call(
            &router,
            "POST",
            "/$procs/biDeposit",
            wire,
            &[],
            Some(&json!({"args": {"accountId": "123456789", "deltaE8": "1"}})),
        )
        .await
        .expect(StatusCode::NOT_FOUND);
    }
    assert_eq!(
        db_i64(
            pool,
            "SELECT amount_e8 FROM bi_accounts WHERE id = $1",
            i64::MAX
        )
        .await,
        before,
        "a refused or failed isolated call must not write"
    );
}

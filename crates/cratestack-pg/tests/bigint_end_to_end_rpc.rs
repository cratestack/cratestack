//! ADR 0019 PR B (B11): the RPC transport counterpart of `bigint_end_to_end.rs`.
//! `POST /rpc/{op_id}` (unary) and `POST /rpc/batch`, JSON and CBOR, real
//! Postgres, at `i64::MAX`, `i64::MIN` and `2^53 + 1`.
//!
//! Transport parity (`CLAUDE.md`): REST and RPC ship together. The unary path
//! re-enters the REST parsing through a synthesized query string; the batch path
//! carries every frame as `serde_json::Value` and re-encodes it, which is where a
//! `BigInt` that had two wire forms would split in two (ADR 0019 D2).
//!
//! Requests are built from the generated typed inputs where that mirrors the
//! Rust client (`RpcGetInput<BigInt>`, `RpcUpdateInput<BigInt, _>`), and from raw
//! JSON/CBOR where the point is to send what a typed client cannot.

use cratestack::axum::Router;
use cratestack::axum::http::StatusCode;
use cratestack::rpc::{
    RpcGetInput, RpcListInput, RpcListPredicate, RpcPkInput, RpcRequest, RpcUpdateInput,
};
use cratestack::serde_json::{Value as Json, json};
use cratestack::sqlx::PgPool;
use cratestack::{BigInt, CodecSet, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use serde::Serialize;

include_server_schema!("tests/fixtures/bigint_end_to_end_rpc.cstack", db = Postgres);

mod bigint_support;
mod support;

use bigint_support::{
    ABOVE_2_53, BOUNDARY, I64_MAX, I64_MIN, Reply, Wire, call_raw, cbor_map, cbor_text, db_i64,
    has_cbor_integer, has_cbor_text, operator_auth, reset,
};
use cratestack::CratestackCodec;
use cratestack_schema::procedures::{
    rpc_bi_deposit, rpc_bi_echo, rpc_bi_find_accounts, rpc_bi_next,
};
use cratestack_schema::{
    CreateRpcBiAccountInput, IsolatedCratestack, RpcBiAccount, RpcBiEcho, UpdateRpcBiAccountInput,
    build_rpc_bi_account_query_from_find_many, rpc_bi_account,
};
use support::pg;

const SCHEMA: &str = include_str!("fixtures/bigint_end_to_end_rpc.cstack");
const TABLES: &str = "rpc_bi_players, rpc_bi_teams, rpc_bi_limits, rpc_bi_ledgers, rpc_bi_accounts";
const TWO_53: &str = "9007199254740992";
const TWO_53_PLUS_2: &str = "9007199254740994";

#[derive(Clone)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn rpc_bi_echo(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: rpc_bi_echo::Args,
        _authorized: rpc_bi_echo::Authorized,
    ) -> Result<rpc_bi_echo::Output, CratestackError> {
        Ok(RpcBiEcho {
            amountE8: args.args.amountE8,
            feeE8: args.args.feeE8,
            tagsE8: args.args.tagsE8,
        })
    }

    async fn rpc_bi_next(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: rpc_bi_next::Args,
        _authorized: rpc_bi_next::Authorized,
    ) -> Result<rpc_bi_next::Output, CratestackError> {
        args.valueE8
            .checked_add(BigInt::new(1))
            .ok_or_else(|| CratestackError::Validation("valueE8 overflows".to_owned()))
    }

    async fn rpc_bi_find_accounts(
        &self,
        db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: rpc_bi_find_accounts::Args,
        _authorized: rpc_bi_find_accounts::Authorized,
    ) -> Result<rpc_bi_find_accounts::Output, CratestackError> {
        build_rpc_bi_account_query_from_find_many(db, &args.query)
            .order_by(rpc_bi_account::id().asc())
            .run(ctx)
            .await
    }

    async fn rpc_bi_deposit(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: rpc_bi_deposit::Args,
        _authorized: rpc_bi_deposit::Authorized,
    ) -> Result<rpc_bi_deposit::Output, CratestackError> {
        let account = db
            .rpc_bi_account()
            .find_unique(args.args.accountId)
            .run(ctx)
            .await?
            .ok_or_else(|| CratestackError::NotFound("no such account".to_owned()))?;
        let amount = account
            .amountE8
            .checked_add(args.args.deltaE8)
            .ok_or_else(|| CratestackError::Validation("amountE8 overflows".to_owned()))?;
        db.rpc_bi_account()
            .update(args.args.accountId)
            .set(UpdateRpcBiAccountInput {
                amountE8: Some(amount),
                ..Default::default()
            })
            .run(ctx)
            .await
    }
}

fn router(pool: &PgPool) -> Router {
    cratestack_schema::axum::rpc_router(
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

/// A typed value encoded exactly as the Rust client would encode it.
fn enc<T: Serialize>(wire: Wire, value: &T) -> Vec<u8> {
    match wire {
        Wire::Json => JsonCodec.encode(value),
        Wire::Cbor => CborCodec.encode(value),
    }
    .expect("encode typed input")
}

async fn unary(router: &Router, wire: Wire, op: &str, body: Vec<u8>) -> Reply {
    unary_with(router, wire, op, &[], body).await
}

async fn unary_with(
    router: &Router,
    wire: Wire,
    op: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Reply {
    call_raw(
        router,
        "POST",
        &format!("/rpc/{op}"),
        wire,
        headers,
        Some(body),
    )
    .await
}

async fn unary_json(router: &Router, wire: Wire, op: &str, input: &Json) -> Reply {
    unary(router, wire, op, wire.encode(input)).await
}

/// `POST /rpc/batch` with `(id, op, input)` frames; the answer is the array of
/// frames, each `{id, output}` or `{id, error: {code, message}}`.
async fn batch(router: &Router, wire: Wire, frames: &[(u64, &str, Json)]) -> (Reply, Vec<Json>) {
    let requests: Vec<RpcRequest> = frames
        .iter()
        .map(|(id, op, input)| RpcRequest {
            id: *id,
            op: (*op).to_owned(),
            input: input.clone(),
            idem: None,
        })
        .collect();
    let reply = call_raw(
        router,
        "POST",
        "/rpc/batch",
        wire,
        &[],
        Some(enc(wire, &requests)),
    )
    .await
    .expect(StatusCode::OK);
    let frames = reply
        .json()
        .as_array()
        .expect("batch answers an array")
        .clone();
    (reply, frames)
}

fn frame_for(frames: &[Json], id: u64) -> &Json {
    frames
        .iter()
        .find(|frame| frame["id"] == id)
        .unwrap_or_else(|| panic!("no frame {id} in {frames:?}"))
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

fn ids_of(rows: &Json) -> Vec<String> {
    rows.as_array()
        .unwrap_or_else(|| panic!("a list is an array: {rows}"))
        .iter()
        .map(|row| {
            row["id"]
                .as_str()
                .unwrap_or_else(|| panic!("a BigInt id is a string: {row}"))
                .to_owned()
        })
        .collect()
}

const ROWS: [(&str, Option<&str>); 5] = [
    (I64_MIN, None),
    (TWO_53, Some(TWO_53)),
    (ABOVE_2_53, Some(ABOVE_2_53)),
    (TWO_53_PLUS_2, None),
    (I64_MAX, Some(I64_MAX)),
];

async fn seed(pool: &PgPool) {
    for (text, fee) in ROWS {
        let n: i64 = text.parse().expect("fits i64");
        let fee: Option<i64> = fee.map(|f| f.parse().expect("fits i64"));
        cratestack::sqlx::query(
            "INSERT INTO rpc_bi_accounts (id, amount_e8, fee_e8, label) VALUES ($1, $1, $2, 's')",
        )
        .bind(n)
        .bind(fee)
        .execute(pool)
        .await
        .expect("seed row");
    }
}

// ---------------------------------------------------------------------------
// the shapes a Rust client sends
// ---------------------------------------------------------------------------

#[test]
fn the_generated_rpc_inputs_carry_a_bigint_key_as_a_string() {
    let get = RpcGetInput {
        id: BigInt::MAX,
        ..Default::default()
    };
    assert_eq!(
        enc(Wire::Json, &get),
        format!(r#"{{"id":"{I64_MAX}"}}"#).into_bytes()
    );
    assert_eq!(
        enc(Wire::Cbor, &get),
        cbor_map(&[("id", cbor_text(I64_MAX))]),
        "an RpcGetInput key is a CBOR text string, 0x73 and 19 digits for i64::MAX"
    );
    let pk = RpcPkInput { id: BigInt::MIN };
    assert_eq!(
        enc(Wire::Cbor, &pk),
        cbor_map(&[("id", cbor_text(I64_MIN))])
    );

    let update = RpcUpdateInput {
        id: BigInt::new(9_007_199_254_740_993),
        patch: UpdateRpcBiAccountInput {
            amountE8: Some(BigInt::MAX),
            ..Default::default()
        },
    };
    assert_eq!(
        enc(Wire::Cbor, &update),
        cbor_map(&[
            ("id", cbor_text(ABOVE_2_53)),
            ("patch", cbor_map(&[("amountE8", cbor_text(I64_MAX))])),
        ])
    );
}

// ---------------------------------------------------------------------------
// unary: create / get / update / delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rpc_unary_crud_round_trips_the_boundary_values_on_both_codecs() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    for wire in Wire::BOTH {
        for (text, n) in BOUNDARY {
            let id = BigInt::new(n);
            let created = unary(
                &router,
                wire,
                "model.RpcBiAccount.create",
                enc(
                    wire,
                    &CreateRpcBiAccountInput {
                        id,
                        amountE8: id,
                        feeE8: Some(id),
                        label: "x".to_owned(),
                    },
                ),
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
                assert!(has_cbor_text(&created.bytes, text));
                assert!(
                    !has_cbor_integer(&created.bytes, n),
                    "{text}: CBOR integer on the wire"
                );
            }
            assert_eq!(
                db_i64(
                    pool,
                    "SELECT amount_e8 FROM rpc_bi_accounts WHERE id = $1",
                    n
                )
                .await,
                n
            );

            let got = unary(
                &router,
                wire,
                "model.RpcBiAccount.get",
                enc(
                    wire,
                    &RpcGetInput {
                        id,
                        ..Default::default()
                    },
                ),
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(got.json()["amountE8"], Json::String(text.to_owned()));

            let projected = unary(
                &router,
                wire,
                "model.RpcBiAccount.get",
                enc(
                    wire,
                    &RpcGetInput {
                        id,
                        fields: Some(vec!["id".to_owned(), "amountE8".to_owned()]),
                        ..Default::default()
                    },
                ),
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(projected.json().as_object().expect("object").len(), 2);
            assert_eq!(projected.json()["id"], Json::String(text.to_owned()));
            if wire == Wire::Cbor {
                assert!(!has_cbor_integer(&projected.bytes, n));
            }

            let other = BigInt::new(
                BOUNDARY[(BOUNDARY.iter().position(|b| b.0 == text).unwrap() + 1) % 3].1,
            );
            let updated = unary(
                &router,
                wire,
                "model.RpcBiAccount.update",
                enc(
                    wire,
                    &RpcUpdateInput {
                        id,
                        patch: UpdateRpcBiAccountInput {
                            amountE8: Some(other),
                            feeE8: Some(None),
                            ..Default::default()
                        },
                    },
                ),
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(updated.json()["amountE8"], Json::String(other.to_string()));
            assert_eq!(
                updated.json()["feeE8"],
                Json::Null,
                "{wire:?}: Some(None) clears"
            );
            assert_eq!(
                db_i64(
                    pool,
                    "SELECT amount_e8 FROM rpc_bi_accounts WHERE id = $1",
                    n
                )
                .await,
                other.get()
            );

            let deleted = unary(
                &router,
                wire,
                "model.RpcBiAccount.delete",
                enc(wire, &RpcPkInput { id }),
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(deleted.json()["id"], Json::String(text.to_owned()));
            unary(
                &router,
                wire,
                "model.RpcBiAccount.get",
                enc(
                    wire,
                    &RpcGetInput {
                        id,
                        ..Default::default()
                    },
                ),
            )
            .await
            .expect(StatusCode::NOT_FOUND);
        }
    }
}

#[tokio::test]
async fn rpc_refuses_a_number_where_a_bigint_is_expected() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        let numbers = [
            ("model.RpcBiAccount.get", json!({"id": 5})),
            (
                "model.RpcBiAccount.get",
                json!({"id": 9_007_199_254_740_993_i64}),
            ),
            ("model.RpcBiAccount.delete", json!({"id": 5})),
            (
                "model.RpcBiAccount.update",
                json!({"id": 5, "patch": {"label": "z"}}),
            ),
            (
                "model.RpcBiAccount.update",
                json!({"id": I64_MAX, "patch": {"amountE8": 5}}),
            ),
            (
                "model.RpcBiAccount.create",
                json!({"id": "1", "amountE8": 5, "label": "x"}),
            ),
            (
                "model.RpcBiAccount.create",
                json!({"id": "1", "amountE8": "1", "feeE8": 5, "label": "x"}),
            ),
            ("procedure.rpcBiNext", json!({"valueE8": 5})),
            (
                "procedure.rpcBiEcho",
                json!({"args": {"amountE8": "1", "feeE8": null, "tagsE8": [1]}}),
            ),
        ];
        for (op, input) in numbers {
            let reply = unary_json(&router, wire, op, &input).await;
            assert_eq!(
                reply.status,
                StatusCode::BAD_REQUEST,
                "{wire:?} {op} {input}"
            );
            assert_eq!(
                reply.json()["code"],
                "invalid_argument",
                "{wire:?} {op} {input}"
            );
        }
        for bad in ["+5", "007", "-0", " 1", "9223372036854775808"] {
            let reply = unary_json(
                &router,
                wire,
                "model.RpcBiAccount.get",
                &json!({ "id": bad }),
            )
            .await;
            assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{wire:?} id {bad:?}");
        }
    }
    // A CBOR integer and a CBOR bignum, which JSON cannot even express.
    let be = [0x48, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    for (label, item) in [
        ("uint", vec![0x05]),
        ("nint", vec![0x20]),
        ("tag 2", [&[0xc2][..], &be].concat()),
        ("tag 3", [&[0xc3][..], &be].concat()),
    ] {
        let reply = unary(
            &router,
            Wire::Cbor,
            "model.RpcBiAccount.get",
            cbor_map(&[("id", item)]),
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "CBOR {label}");
    }
    let rows: i64 = cratestack::sqlx::query_scalar("SELECT count(*) FROM rpc_bi_accounts")
        .fetch_one(&test_pg.pool)
        .await
        .expect("count");
    assert_eq!(rows, 5, "no refused call may write");
}

// ---------------------------------------------------------------------------
// unary: list, filters, fields
// ---------------------------------------------------------------------------

fn list(filters: &[(&str, &str)]) -> RpcListInput {
    RpcListInput {
        sort: Some("id".to_owned()),
        filters: filters
            .iter()
            .map(|(key, value)| RpcListPredicate {
                key: (*key).to_owned(),
                value: (*value).to_owned(),
            })
            .collect(),
        ..Default::default()
    }
}

#[tokio::test]
async fn rpc_list_filters_compare_bigint_numerically_at_the_boundaries() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    let in_both_ends = format!("{I64_MAX},{I64_MIN}");
    let cases: Vec<(RpcListInput, Vec<String>)> = vec![
        (
            list(&[("amountE8__gt", ABOVE_2_53)]),
            strings(&[TWO_53_PLUS_2, I64_MAX]),
        ),
        (
            list(&[("amountE8__gte", ABOVE_2_53)]),
            strings(&[ABOVE_2_53, TWO_53_PLUS_2, I64_MAX]),
        ),
        (
            list(&[("amountE8__lt", ABOVE_2_53)]),
            strings(&[I64_MIN, TWO_53]),
        ),
        (
            list(&[("amountE8__lte", ABOVE_2_53)]),
            strings(&[I64_MIN, TWO_53, ABOVE_2_53]),
        ),
        (list(&[("amountE8", ABOVE_2_53)]), strings(&[ABOVE_2_53])),
        (
            list(&[("amountE8__ne", ABOVE_2_53)]),
            strings(&[I64_MIN, TWO_53, TWO_53_PLUS_2, I64_MAX]),
        ),
        (
            list(&[("amountE8__in", &in_both_ends)]),
            strings(&[I64_MIN, I64_MAX]),
        ),
        (
            list(&[("feeE8__isNull", "true")]),
            strings(&[I64_MIN, TWO_53_PLUS_2]),
        ),
        (
            list(&[("amountE8__gt", TWO_53), ("amountE8__lt", TWO_53_PLUS_2)]),
            strings(&[ABOVE_2_53]),
        ),
        (
            RpcListInput {
                where_expr: Some(format!("not(amountE8={ABOVE_2_53})")),
                sort: Some("id".to_owned()),
                ..Default::default()
            },
            strings(&[I64_MIN, TWO_53, TWO_53_PLUS_2, I64_MAX]),
        ),
    ];
    for wire in Wire::BOTH {
        for (input, expected) in &cases {
            let reply = unary(&router, wire, "model.RpcBiAccount.list", enc(wire, input))
                .await
                .expect(StatusCode::OK);
            assert_eq!(&ids_of(&reply.json()), expected, "{wire:?} {input:?}");
        }
    }
}

#[tokio::test]
async fn rpc_list_with_fields_and_descending_sort_is_numeric() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        let reply = unary(
            &router,
            wire,
            "model.RpcBiAccount.list",
            enc(
                wire,
                &RpcListInput {
                    fields: Some(vec!["id".to_owned(), "feeE8".to_owned()]),
                    sort: Some("-amountE8".to_owned()),
                    ..Default::default()
                },
            ),
        )
        .await
        .expect(StatusCode::OK);
        let rows = reply.json();
        assert_eq!(
            ids_of(&rows),
            strings(&[I64_MAX, TWO_53_PLUS_2, ABOVE_2_53, TWO_53, I64_MIN]),
            "{wire:?}"
        );
        for row in rows.as_array().expect("array") {
            assert_eq!(
                row.as_object().expect("object").len(),
                2,
                "{wire:?}: ?fields= parity"
            );
        }
        if wire == Wire::Cbor {
            for (text, n) in BOUNDARY {
                assert!(has_cbor_text(&reply.bytes, text));
                assert!(!has_cbor_integer(&reply.bytes, n));
            }
        }
    }
}

#[tokio::test]
async fn rpc_list_refuses_a_filter_value_that_is_not_a_canonical_bigint() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for bad in ["9223372036854775808", "abc", "007", "+5", "-0", "1.5", ""] {
        for key in ["amountE8", "amountE8__gt", "amountE8__in"] {
            let reply = unary(
                &router,
                Wire::Json,
                "model.RpcBiAccount.list",
                enc(Wire::Json, &list(&[(key, bad)])),
            )
            .await;
            assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{key}={bad:?}");
            assert_eq!(reply.json()["code"], "invalid_argument");
        }
    }
}

// ---------------------------------------------------------------------------
// batch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rpc_batch_round_trips_bigint_frames_on_both_codecs() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    for wire in Wire::BOTH {
        let mut frames: Vec<(u64, &str, Json)> = Vec::new();
        for (i, (text, _)) in BOUNDARY.iter().enumerate() {
            frames.push((
                10 + i as u64,
                "model.RpcBiAccount.create",
                json!({"id": text, "amountE8": text, "feeE8": text, "label": "b"}),
            ));
        }
        let (reply, answers) = batch(&router, wire, &frames).await;
        for (i, (text, n)) in BOUNDARY.iter().enumerate() {
            let frame = frame_for(&answers, 10 + i as u64);
            assert!(frame["error"].is_null(), "{wire:?} create {text}: {frame}");
            assert_eq!(
                frame["output"]["id"],
                Json::String((*text).to_owned()),
                "{wire:?}"
            );
            assert_eq!(
                frame["output"]["amountE8"],
                Json::String((*text).to_owned())
            );
            assert_eq!(
                db_i64(pool, "SELECT fee_e8 FROM rpc_bi_accounts WHERE id = $1", *n).await,
                *n
            );
        }
        if wire == Wire::Cbor {
            // The answer went serde_json::Value -> CBOR: still a text string.
            for (text, n) in BOUNDARY {
                assert!(
                    has_cbor_text(&reply.bytes, text),
                    "{text}: batch output is not a text string"
                );
                assert!(
                    !has_cbor_integer(&reply.bytes, n),
                    "{text}: batch output is a CBOR integer"
                );
            }
        }

        // get, projection, update, list and both procedures, in one batch.
        let frames: Vec<(u64, &str, Json)> = vec![
            (1, "model.RpcBiAccount.get", json!({"id": I64_MAX})),
            (
                2,
                "model.RpcBiAccount.get",
                json!({"id": I64_MIN, "fields": ["id"]}),
            ),
            (
                3,
                "model.RpcBiAccount.update",
                json!({"id": ABOVE_2_53, "patch": {"amountE8": I64_MAX}}),
            ),
            (
                4,
                "model.RpcBiAccount.list",
                json!({"filters": [{"key": "amountE8__gt", "value": ABOVE_2_53}], "sort": "id"}),
            ),
            (
                5,
                "procedure.rpcBiEcho",
                json!({"args": {"amountE8": ABOVE_2_53, "feeE8": null, "tagsE8": [I64_MIN, I64_MAX]}}),
            ),
            (6, "procedure.rpcBiNext", json!({"valueE8": TWO_53})),
        ];
        let (reply, answers) = batch(&router, wire, &frames).await;
        assert_eq!(frame_for(&answers, 1)["output"]["amountE8"], I64_MAX);
        assert_eq!(frame_for(&answers, 2)["output"], json!({"id": I64_MIN}));
        assert_eq!(frame_for(&answers, 3)["output"]["amountE8"], I64_MAX);
        assert_eq!(
            ids_of(&frame_for(&answers, 4)["output"]),
            strings(&[ABOVE_2_53, I64_MAX])
        );
        assert_eq!(
            frame_for(&answers, 5)["output"],
            json!({"amountE8": ABOVE_2_53, "feeE8": null, "tagsE8": [I64_MIN, I64_MAX]})
        );
        assert_eq!(
            frame_for(&answers, 6)["output"],
            Json::String(ABOVE_2_53.to_owned())
        );
        if wire == Wire::Cbor {
            assert!(!has_cbor_integer(&reply.bytes, i64::MAX));
            assert!(!has_cbor_integer(&reply.bytes, i64::MIN));
        }

        // Put the row back so the next wire starts from the same state.
        cratestack::sqlx::query("DELETE FROM rpc_bi_accounts")
            .execute(pool)
            .await
            .expect("clear");
    }
}

#[tokio::test]
async fn rpc_batch_gives_a_number_its_own_error_frame_and_does_not_poison_the_rest() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        let frames: Vec<(u64, &str, Json)> = vec![
            (1, "model.RpcBiAccount.get", json!({"id": I64_MAX})),
            (2, "model.RpcBiAccount.get", json!({"id": 5})),
            (3, "model.RpcBiAccount.get", json!({"id": "007"})),
            (
                4,
                "model.RpcBiAccount.update",
                json!({"id": I64_MAX, "patch": {"amountE8": 5}}),
            ),
            (5, "procedure.rpcBiNext", json!({"valueE8": 7})),
            (6, "model.RpcBiAccount.get", json!({"id": I64_MIN})),
        ];
        let (_, answers) = batch(&router, wire, &frames).await;
        assert_eq!(frame_for(&answers, 1)["output"]["id"], I64_MAX);
        assert_eq!(frame_for(&answers, 6)["output"]["id"], I64_MIN);
        for id in [2, 3, 4, 5] {
            let frame = frame_for(&answers, id);
            assert_eq!(
                frame["error"]["code"], "invalid_argument",
                "{wire:?} frame {id} must be refused, got {frame}"
            );
            assert!(frame["output"].is_null(), "{wire:?} frame {id}: {frame}");
        }
    }
    assert_eq!(
        db_i64(
            &test_pg.pool,
            "SELECT amount_e8 FROM rpc_bi_accounts WHERE id = $1",
            i64::MAX
        )
        .await,
        i64::MAX,
        "a refused update frame must not change the row"
    );
}

// ---------------------------------------------------------------------------
// procedures, FindMany, version, range, relations over RPC
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rpc_procedures_take_and_return_bigint_scalars_options_and_lists() {
    fixture!(test_pg, router);
    let _ = &test_pg;
    for wire in Wire::BOTH {
        for (text, n) in BOUNDARY {
            let reply = unary_json(
                &router,
                wire,
                "procedure.rpcBiEcho",
                &json!({"args": {"amountE8": text, "feeE8": text, "tagsE8": [text, I64_MIN]}}),
            )
            .await
            .expect(StatusCode::OK);
            assert_eq!(
                reply.json(),
                json!({"amountE8": text, "feeE8": text, "tagsE8": [text, I64_MIN]}),
                "{wire:?}"
            );
            if wire == Wire::Cbor {
                assert!(!has_cbor_integer(&reply.bytes, n));
            }
        }
        let next = unary_json(
            &router,
            wire,
            "procedure.rpcBiNext",
            &json!({"valueE8": TWO_53}),
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(next.json(), Json::String(ABOVE_2_53.to_owned()));
        if wire == Wire::Cbor {
            assert_eq!(
                next.bytes,
                cbor_text(ABOVE_2_53),
                "a bare BigInt return is a lone text string"
            );
        }
    }
}

#[tokio::test]
async fn rpc_find_many_where_on_a_bigint_field_really_filters() {
    fixture!(test_pg, router);
    seed(&test_pg.pool).await;
    for wire in Wire::BOTH {
        let find = |query: Json| {
            let router = router.clone();
            async move {
                unary_json(
                    &router,
                    wire,
                    "procedure.rpcBiFindAccounts",
                    &json!({ "query": query }),
                )
                .await
                .expect(StatusCode::OK)
            }
        };
        assert_eq!(
            ids_of(&find(json!({})).await.json()).len(),
            5,
            "{wire:?}: unfiltered"
        );
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
        ];
        for (query, expected) in cases {
            assert_eq!(
                ids_of(&find(query.clone()).await.json()),
                expected,
                "{wire:?} {query}"
            );
        }
        unary_json(
            &router,
            wire,
            "procedure.rpcBiFindAccounts",
            &json!({"query": {"where": {"amountE8": {"gt": 5}}}}),
        )
        .await
        .expect(StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn rpc_update_honours_an_exact_if_match_on_a_bigint_version() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    cratestack::sqlx::query(
        "INSERT INTO rpc_bi_ledgers (id, balance_e8, version) VALUES (1, 1, 9007199254740993)",
    )
    .execute(pool)
    .await
    .expect("seed");
    let version = |pool: PgPool| async move {
        cratestack::sqlx::query_scalar::<_, i64>("SELECT version FROM rpc_bi_ledgers WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("version")
    };
    for wire in Wire::BOTH {
        let current = version(pool.clone()).await;
        let patch = json!({"id": "1", "patch": {"balanceE8": I64_MAX}});

        unary_json(&router, wire, "model.RpcBiLedger.update", &patch)
            .await
            .expect(StatusCode::PRECONDITION_FAILED);
        let off_by_one = format!("\"{}\"", current - 1);
        unary_with(
            &router,
            wire,
            "model.RpcBiLedger.update",
            &[("if-match", &off_by_one)],
            wire.encode(&patch),
        )
        .await
        .expect(StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            version(pool.clone()).await,
            current,
            "{wire:?}: a stale If-Match changes nothing"
        );

        let exact = format!("\"{current}\"");
        let bumped = unary_with(
            &router,
            wire,
            "model.RpcBiLedger.update",
            &[("if-match", &exact)],
            wire.encode(&patch),
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(
            bumped.etag(),
            Some(format!("\"{}\"", current + 1)),
            "{wire:?}"
        );
        assert_eq!(
            bumped.json()["version"],
            Json::String((current + 1).to_string())
        );
        assert_eq!(version(pool.clone()).await, current + 1);
    }
}

#[tokio::test]
async fn rpc_range_on_a_bigint_is_invalid_argument_and_a_check_constraint() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    for wire in Wire::BOTH {
        for (field, value) in [
            ("smallE8", "-1"),
            ("smallE8", "1000001"),
            ("wideE8", I64_MAX),
            ("wideE8", I64_MIN),
        ] {
            let mut input = json!({"id": 1, "smallE8": "0", "wideE8": "0"});
            input[field] = Json::String(value.to_owned());
            let reply = unary_json(&router, wire, "model.RpcBiLimit.create", &input).await;
            assert_eq!(
                reply.status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{wire:?} {field}={value}"
            );
            assert_eq!(reply.json()["code"], "invalid_argument");
            let message = reply.json()["message"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            assert!(
                message.contains(field),
                "{wire:?}: the message must name `{field}`: {message}"
            );
        }
    }
    unary_json(
        &router,
        Wire::Json,
        "model.RpcBiLimit.create",
        &json!({"id": 2, "smallE8": "1000000", "wideE8": "-9223372036854775807"}),
    )
    .await
    .expect(StatusCode::CREATED);

    let error = cratestack::sqlx::query(
        "INSERT INTO rpc_bi_limits (id, small_e8, wide_e8) VALUES (9, -1, 0)",
    )
    .execute(pool)
    .await
    .expect_err("the CHECK must refuse it");
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("23514")
    );
}

#[tokio::test]
async fn rpc_include_loads_a_bigint_foreign_key_in_both_directions() {
    fixture!(test_pg, router);
    for (id, name) in [(I64_MAX, "max"), (ABOVE_2_53, "mid")] {
        unary_json(
            &router,
            Wire::Json,
            "model.RpcBiTeam.create",
            &json!({"id": id, "name": name}),
        )
        .await
        .expect(StatusCode::CREATED);
    }
    for (id, team) in [(I64_MIN, I64_MAX), (TWO_53, I64_MAX), ("1", ABOVE_2_53)] {
        unary_json(
            &router,
            Wire::Cbor,
            "model.RpcBiPlayer.create",
            &json!({"id": id, "teamId": team, "scoreE8": "1"}),
        )
        .await
        .expect(StatusCode::CREATED);
    }
    for wire in Wire::BOTH {
        let player = unary(
            &router,
            wire,
            "model.RpcBiPlayer.get",
            enc(
                wire,
                &RpcGetInput {
                    id: BigInt::MIN,
                    include: Some(vec!["team".to_owned()]),
                    ..Default::default()
                },
            ),
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(player.json()["team"]["id"], I64_MAX, "{wire:?}");
        assert_eq!(player.json()["teamId"], I64_MAX);

        let teams = unary(
            &router,
            wire,
            "model.RpcBiTeam.list",
            enc(
                wire,
                &RpcListInput {
                    include: Some(vec!["players".to_owned()]),
                    sort: Some("id".to_owned()),
                    ..Default::default()
                },
            ),
        )
        .await
        .expect(StatusCode::OK);
        let counts: Vec<(String, usize)> = teams
            .json()
            .as_array()
            .expect("array")
            .iter()
            .map(|team| {
                (
                    team["id"].as_str().expect("string id").to_owned(),
                    team["players"].as_array().expect("players").len(),
                )
            })
            .collect();
        assert_eq!(
            counts,
            vec![(ABOVE_2_53.to_owned(), 1), (I64_MAX.to_owned(), 2)],
            "{wire:?}"
        );
    }
}

#[test]
fn the_find_many_where_type_is_generated_with_the_bigint_fields() {
    // Compile-time proof that the typed `Where` carries the field (a `Where`
    // that dropped it would fail to build here, not just filter nothing).
    let filter = cratestack::FieldFilterInput::<BigInt> {
        gt: Some(BigInt::new(1)),
        ..Default::default()
    };
    let where_ = cratestack_schema::RpcBiAccountWhere {
        amountE8: Some(filter),
        ..Default::default()
    };
    assert_eq!(where_.to_filters().len(), 1);
    let _: Option<RpcBiAccount> = None;
}

#[tokio::test]
async fn rpc_isolated_procedure_reads_and_writes_a_bigint_key_through_its_transaction() {
    fixture!(test_pg, router);
    let pool = &test_pg.pool;
    seed(pool).await;
    for wire in Wire::BOTH {
        let current = db_i64(
            pool,
            "SELECT amount_e8 FROM rpc_bi_accounts WHERE id = $1",
            9_007_199_254_740_993,
        )
        .await;
        let reply = unary_json(
            &router,
            wire,
            "procedure.rpcBiDeposit",
            &json!({"args": {"accountId": ABOVE_2_53, "deltaE8": "1"}}),
        )
        .await
        .expect(StatusCode::OK);
        assert_eq!(
            reply.json()["amountE8"],
            Json::String((current + 1).to_string()),
            "{wire:?}"
        );
        assert_eq!(
            db_i64(
                pool,
                "SELECT amount_e8 FROM rpc_bi_accounts WHERE id = $1",
                9_007_199_254_740_993
            )
            .await,
            current + 1
        );

        let refused = unary_json(
            &router,
            wire,
            "procedure.rpcBiDeposit",
            &json!({"args": {"accountId": 5, "deltaE8": "1"}}),
        )
        .await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{wire:?}");
    }
    // The same call as a batch frame, next to a get.
    let (_, answers) = batch(
        &router,
        Wire::Cbor,
        &[
            (
                1,
                "procedure.rpcBiDeposit",
                json!({"args": {"accountId": TWO_53, "deltaE8": "1"}}),
            ),
            (2, "procedure.rpcBiNext", json!({"valueE8": TWO_53})),
        ],
    )
    .await;
    assert_eq!(frame_for(&answers, 1)["output"]["amountE8"], ABOVE_2_53);
    assert_eq!(frame_for(&answers, 2)["output"], ABOVE_2_53);
    let committed = unary_json(
        &router,
        Wire::Json,
        "model.RpcBiAccount.get",
        &json!({"id": TWO_53}),
    )
    .await
    .expect(StatusCode::OK);
    assert_eq!(
        committed.json()["amountE8"],
        ABOVE_2_53,
        "the batched isolated write committed"
    );
}

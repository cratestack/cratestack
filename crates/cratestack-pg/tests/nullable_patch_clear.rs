//! cratestack#567 — JSON/CBOR `null` in a PATCH body must clear a
//! nullable column, not silently no-op.
//!
//! Root cause (server-side inbound): `Update{Model}Input` wraps every
//! field in an outer `Option<T>` ("was this field touched"), and a
//! nullable column is *separately* `Option<T>` — a nullable field on an
//! update input is therefore `Option<Option<T>>`. serde-derive's blanket
//! `Option<T>: Deserialize` only peels the outer layer, so an absent key
//! and an explicit JSON/CBOR `null` both collapsed to outer `None`, and
//! `update_sql_value` reads a `None` field as "not present in this
//! update" and skips the column entirely. See
//! `crates/cratestack-core/src/patch.rs` for the fix
//! (`deserialize_double_option`) and `crates/cratestack-macros/src/model/
//! struct_only.rs::struct_field_definition` for where it's wired in.
//!
//! A second, less obvious bug shares the same root cause in reverse: the
//! generated `Update{Model}Input`'s derived `Serialize` had no
//! `skip_serializing_if`, so *every* generated client (Rust/Dart/
//! TypeScript) — which builds a full input struct with
//! `..Default::default()` for untouched fields and serializes the whole
//! struct — was already sending `"field": null` on the wire for fields it
//! never touched. That was harmless before this fix only because the
//! deserialize bug treated it the same as an absent key; fixing
//! deserialization alone would have turned every one of those "untouched"
//! sends into a real "clear this column", a severe regression. The fix
//! pairs `deserialize_double_option` with
//! `#[serde(skip_serializing_if = "Option::is_none")]` on the same field
//! so an untouched field is omitted from the wire entirely.
//! `client_update_omits_untouched_nullable_field_on_the_wire` below
//! proves that half directly.
//!
//! cratestack#663 closes the one arity that fix missed: `struct_field_
//! definition`'s match had arms for `TypeArity::Optional` (above) and
//! `TypeArity::List` (#662), but `TypeArity::Required` fell through to the
//! empty `else` — no `skip_serializing_if` at all — so an untouched
//! `Required`-arity field (`name` in this fixture) still serialized as
//! `"name":null`. `client_update_omits_untouched_required_arity_field_on_
//! the_wire` proves that's fixed; unlike `note`, `name` has no inner
//! "clear" state to preserve (a `NOT NULL` column can't be explicitly
//! cleared), so its fix is a plain `skip_serializing_if`, no double-`Option`
//! needed.
//!
//! Skips quietly when neither `CRATESTACK_TEST_DATABASE_URL` nor
//! `CRATESTACK_USE_TESTCONTAINERS` is set (see `tests/support/pg.rs`).

mod support;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::CratestackCodec;
use support::pg;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/nullable_patch_clear.cstack", db = Postgres);

use cratestack_schema::UpdatePatchClearTargetInput;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, patch_clear_targets")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE patch_clear_targets (
            id BIGINT PRIMARY KEY,
            name TEXT NOT NULL,
            note TEXT
        )",
    )
    .execute(pool)
    .await
    .expect("create items");
}

fn ctx() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("issue-567")
}

#[derive(Clone)]
struct PassThroughAuth;

impl AuthProvider for PassThroughAuth {
    type Error = CratestackError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(ctx()))
    }
}

async fn seed(pool: &cratestack::sqlx::PgPool, id: i64, note: Option<&str>) {
    query("INSERT INTO patch_clear_targets (id, name, note) VALUES ($1, $2, $3)")
        .bind(id)
        .bind("widget")
        .bind(note)
        .execute(pool)
        .await
        .expect("seed item");
}

async fn read_row(pool: &cratestack::sqlx::PgPool, id: i64) -> (String, Option<String>) {
    let row = query("SELECT name, note FROM patch_clear_targets WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read item");
    (
        row.get::<String, _>("name"),
        row.get::<Option<String>, _>("note"),
    )
}

// ───── #1 THE DECISIVE TEST: `null` over the real HTTP PATCH route ──────

#[tokio::test]
async fn patch_null_clears_a_nullable_column_over_json() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool, 1, Some("has a note")).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = cratestack_schema::axum::model_router(cool, (), JsonCodec, PassThroughAuth);

    // Mirrors the issue's own repro shape exactly: `name` genuinely
    // changes in the SAME request as the `null`. This matters for the
    // failure mode, not just realism — a PATCH whose *only* field is a
    // pre-fix-ambiguous `null` produces an empty `sql_values()` and trips
    // the framework's separate "empty patch" 422 guard
    // (`update_empty_patch_preflight`, `cratestack-macros/src/axum/model/
    // prep.rs`), which is a loud, different failure, not the silent
    // no-op #567 is about. Pairing it with a real change is what makes
    // the request pass that guard and reach the actual bug: a `200` that
    // quietly leaves `note` unchanged.
    let response = router
        .oneshot(
            Request::patch("/patch_clear_targets/1")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(r#"{"name": "renamed", "note": null}"#))
                .expect("req"),
        )
        .await
        .expect("patch");
    assert_eq!(response.status(), StatusCode::OK);

    let (name, note) = read_row(pool, 1).await;
    assert_eq!(
        name, "renamed",
        "the other field in the same request must still apply"
    );
    assert_eq!(
        note, None,
        "an explicit JSON `null` on a nullable field must clear the column to SQL NULL"
    );
}

// ───── #2 the three states, distinguishable in one suite ────────────────

#[tokio::test]
async fn absent_key_leaves_a_nullable_column_untouched() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool, 2, Some("keep me")).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = cratestack_schema::axum::model_router(cool, (), JsonCodec, PassThroughAuth);

    let response = router
        .oneshot(
            Request::patch("/patch_clear_targets/2")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(r#"{"name": "renamed"}"#))
                .expect("req"),
        )
        .await
        .expect("patch");
    assert_eq!(response.status(), StatusCode::OK);

    let (name, note) = read_row(pool, 2).await;
    assert_eq!(name, "renamed");
    assert_eq!(
        note,
        Some("keep me".to_owned()),
        "omitting the key must leave the nullable column untouched"
    );
}

#[tokio::test]
async fn explicit_value_sets_a_nullable_column() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool, 3, None).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = cratestack_schema::axum::model_router(cool, (), JsonCodec, PassThroughAuth);

    let response = router
        .oneshot(
            Request::patch("/patch_clear_targets/3")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(r#"{"note": "now set"}"#))
                .expect("req"),
        )
        .await
        .expect("patch");
    assert_eq!(response.status(), StatusCode::OK);

    let (_, note) = read_row(pool, 3).await;
    assert_eq!(note, Some("now set".to_owned()));
}

// ───── #3 non-nullable field is unaffected ───────────────────────────────

#[tokio::test]
async fn omitting_a_non_nullable_field_still_means_untouched() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool, 4, Some("note stays")).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = cratestack_schema::axum::model_router(cool, (), JsonCodec, PassThroughAuth);

    // `name` is non-nullable (`Option<T>`, single layer — not
    // double-Option), so this fixture's fix path doesn't touch it at all.
    // Only `note` is set here; `name` must survive unchanged.
    let response = router
        .oneshot(
            Request::patch("/patch_clear_targets/4")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(r#"{"note": "updated"}"#))
                .expect("req"),
        )
        .await
        .expect("patch");
    assert_eq!(response.status(), StatusCode::OK);

    let (name, note) = read_row(pool, 4).await;
    assert_eq!(
        name, "widget",
        "omitted non-nullable field must be untouched"
    );
    assert_eq!(note, Some("updated".to_owned()));
}

// ───── #4 CBOR carries the same three-state semantics as JSON ───────────
//
// Both codecs go through the exact same generated `Deserialize` impl on
// `UpdatePatchClearTargetInput` (`CratestackCodec::decode<T>` is generic over `T`, and the
// derive-emitted `deserialize_with` lives on the struct itself, not in
// either codec) — this test proves that concretely for CBOR rather than
// relying on that being obvious from reading the code. `decode_rpc_body`/
// `decode_transport_request_for` (`cratestack-axum/src/rpc/codec_helpers
// .rs`, used by both the REST PATCH handler and the RPC `model.<M>.update`
// dispatch arm) select a codec by `Content-Type` and then call this same
// generic `decode::<T>`, so RPC shares this fix automatically too — it has
// no deserialization path of its own for CRUD update inputs.

#[test]
fn cbor_and_json_agree_on_all_three_states() {
    // Typed source structs, one per wire shape a real sender would
    // produce — deliberately NOT routed through `serde_json::Value` as an
    // intermediate: `cratestack-codec-cbor`'s own doc/tests note that
    // `serde_json::Value::Null` mis-encodes via minicbor-serde (calls
    // `serialize_unit()`, decodes back as an empty array, not CBOR
    // `null`), which is a known quirk of that generic type — not of the
    // `Option<T>` fields these structs actually use, which correctly hit
    // `serialize_none()`/CBOR `0xf6` on both codecs.
    #[derive(serde::Serialize)]
    struct SourceAbsent {}
    #[derive(serde::Serialize)]
    struct SourceNull {
        note: Option<i64>,
    }
    #[derive(serde::Serialize)]
    struct SourceValue {
        note: i64,
    }

    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            default,
            deserialize_with = "cratestack_core::deserialize_double_option"
        )]
        note: Option<Option<i64>>,
    }

    fn check<S: serde::Serialize>(label: &str, source: &S, expected: Option<Option<i64>>) {
        let json_bytes = JsonCodec.encode(source).expect("encode json probe");
        let from_json: Probe = JsonCodec.decode(&json_bytes).expect("decode json probe");
        assert_eq!(from_json.note, expected, "JSON mismatch for {label}");

        let cbor_bytes = CborCodec.encode(source).expect("encode cbor probe");
        let from_cbor: Probe = CborCodec.decode(&cbor_bytes).expect("decode cbor probe");
        assert_eq!(from_cbor.note, expected, "CBOR mismatch for {label}");
    }

    check("key absent", &SourceAbsent {}, None);
    check("explicit null", &SourceNull { note: None }, Some(None));
    check("explicit value", &SourceValue { note: 7 }, Some(Some(7)));
}

// ───── #5 the outbound half: an untouched field never hits the wire ─────

#[test]
fn client_update_omits_untouched_nullable_field_on_the_wire() {
    // A caller builds a partial patch the same way every generated client
    // does: set the field(s) it cares about, `..Default::default()` for
    // the rest. `note` here is untouched (outer `None`).
    let input = UpdatePatchClearTargetInput {
        name: Some("renamed".to_owned()),
        note: None,
    };
    let json = serde_json::to_string(&input).expect("serialize update input");
    assert!(
        !json.contains("note"),
        "an untouched nullable field must be OMITTED from the wire \
         (not sent as `null`), otherwise it would be indistinguishable \
         from an explicit clear once the server honours `null` as clear: {json}"
    );

    // An explicit clear DOES have to serialize, as `null` specifically.
    let clearing = UpdatePatchClearTargetInput {
        name: None,
        note: Some(None),
    };
    let json = serde_json::to_string(&clearing).expect("serialize clearing input");
    assert!(
        json.contains("\"note\":null"),
        "an explicit clear must serialize the field as JSON null: {json}"
    );

    // And a genuine new value serializes as that value.
    let setting = UpdatePatchClearTargetInput {
        name: None,
        note: Some(Some("hi".to_owned())),
    };
    let json = serde_json::to_string(&setting).expect("serialize setting input");
    assert!(
        json.contains("\"note\":\"hi\""),
        "a genuine new value must serialize as that value: {json}"
    );
}

// ───── #6 cratestack#663: an untouched `Required`-arity field is ALSO
// omitted from the wire, not just `Optional`-arity ones ────────────────────

#[test]
fn client_update_omits_untouched_required_arity_field_on_the_wire() {
    // `name` is `Required` arity (a `NOT NULL` column, single-layer
    // `Option<T>` on the patch struct — no inner "clear" state at all).
    // Before cratestack#663's fix, `struct_field_definition`'s match had no
    // arm for `TypeArity::Required` at all — it fell through to the empty
    // `else`, carrying no `skip_serializing_if` — so an untouched `name`
    // serialized as `"name":null`, the same shape #567 fixed for
    // `Optional`-arity fields. `note` here is genuinely touched (set to a
    // real value) so the request isn't vacuous.
    let input = UpdatePatchClearTargetInput {
        name: None,
        note: Some(Some("hi".to_owned())),
    };
    let json = serde_json::to_string(&input).expect("serialize update input");
    assert!(
        !json.contains("name"),
        "an untouched Required-arity field must be omitted from the wire, \
         not sent as `null`, matching Optional-arity fields: {json}"
    );
}

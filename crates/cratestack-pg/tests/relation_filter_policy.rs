//! GHSA-p55v-6xv5-93p3: a relation filter or relation sort must not let a
//! caller observe a related row the related model's read policy (or
//! `@@soft_delete`) hides from them. The reference behaviour is
//! `?include=`: a hidden related row reads as nonexistent (`null` /
//! absent). So a to-one filter never matches it (`ne` and `isNull`
//! included), `none`/`every` over hidden-only children are vacuously
//! true, and a relation sort key reads as `NULL`.
//!
//! Every check that expects "nothing" is paired with a positive control
//! showing the same shape still works against a related row the caller
//! *can* read — otherwise an over-broad fix (or a broken query) would pass.
//! The RPC transport has its own file, `relation_filter_policy_rpc.rs`.

mod support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::Request;
use cratestack::include_server_schema;
use cratestack::serde_json::{self, Value as Json};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use support::pg;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/relation_filter_policy.cstack",
    db = Postgres
);

#[derive(Clone)]
struct CallerOne;

impl AuthProvider for CallerOne {
    type Error = cratestack::CratestackError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(caller_one()))
    }
}

fn caller_one() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

type Router = cratestack::axum::Router;

async fn get(router: &Router, path: &str) -> Json {
    let mut request = Request::get(path).body(Body::empty()).unwrap();
    request
        .headers_mut()
        .insert("accept", JsonCodec::CONTENT_TYPE.parse().unwrap());
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8_lossy(&bytes).into_owned();
    assert_eq!(status, 200, "{path} -> {body}");
    serde_json::from_str(&body).unwrap()
}

/// Row ids in response order (for sorts).
async fn ids(router: &Router, path: &str) -> Vec<i64> {
    get(router, path)
        .await
        .as_array()
        .unwrap_or_else(|| panic!("{path}: expected an array"))
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect()
}

/// Matched row ids, ascending (for filters, which carry no order).
async fn matched(router: &Router, path: &str) -> Vec<i64> {
    let mut ids = ids(router, path).await;
    ids.sort_unstable();
    ids
}

const NONE: Vec<i64> = Vec::new();

async fn setup() -> Option<(pg::TestPg, cratestack_schema::Cratestack, Router)> {
    let test_pg = pg::connect_or_skip().await?;
    for statement in [
        "DROP TABLE IF EXISTS rf_outers, rf_tags, rf_links, rf_childs, rf_hiddens, rf_denieds, \
         rf_owneds, rf_softs, rf_internals, rf_publics, rf_paged_links",
        "CREATE TABLE rf_hiddens (id BIGINT PRIMARY KEY, code TEXT NOT NULL, \
         score BIGINT NOT NULL, note TEXT NULL, secret TEXT NOT NULL)",
        "CREATE TABLE rf_childs (id BIGINT PRIMARY KEY, hidden_id BIGINT NOT NULL, label TEXT NOT NULL)",
        "CREATE TABLE rf_denieds (id BIGINT PRIMARY KEY, code TEXT NOT NULL)",
        "CREATE TABLE rf_owneds (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, code TEXT NOT NULL)",
        "CREATE TABLE rf_softs (id BIGINT PRIMARY KEY, code TEXT NOT NULL, deleted_at TIMESTAMPTZ NULL)",
        "CREATE TABLE rf_internals (id BIGINT PRIMARY KEY, code TEXT NOT NULL)",
        "CREATE TABLE rf_links (id BIGINT PRIMARY KEY, hidden_id BIGINT NOT NULL, \
         denied_id BIGINT NOT NULL, owned_id BIGINT NOT NULL, soft_id BIGINT NOT NULL, \
         internal_id BIGINT NOT NULL, public_id BIGINT NOT NULL)",
        "CREATE TABLE rf_tags (id BIGINT PRIMARY KEY, link_id BIGINT NOT NULL, label TEXT NOT NULL)",
        "CREATE TABLE rf_publics (id BIGINT PRIMARY KEY, code TEXT NOT NULL)",
        "CREATE TABLE rf_outers (id BIGINT PRIMARY KEY, link_id BIGINT NOT NULL)",
        "CREATE TABLE rf_paged_links (id BIGINT PRIMARY KEY, hidden_id BIGINT NOT NULL, \
         owned_id BIGINT NOT NULL)",
        "INSERT INTO rf_hiddens VALUES (40, 'CODE-A', 10, NULL, 'S1'), (41, 'CODE-B', 20, 'n', 'S2')",
        "INSERT INTO rf_childs VALUES (70, 40, 'kid-a'), (71, 41, 'kid-b')",
        "INSERT INTO rf_denieds VALUES (80, 'D-A'), (81, 'D-B')",
        "INSERT INTO rf_owneds VALUES (60, 1, 'MINE'), (61, 2, 'THEIRS')",
        "INSERT INTO rf_softs VALUES (90, 'S-LIVE', NULL), (91, 'S-GONE', now())",
        "INSERT INTO rf_internals VALUES (95, 'I-A'), (96, 'I-B')",
        "INSERT INTO rf_links VALUES (50, 40, 80, 60, 90, 95, 97), \
         (51, 41, 81, 61, 91, 96, 98)",
        "INSERT INTO rf_tags VALUES (110, 50, 't-a'), (111, 51, 't-b')",
        "INSERT INTO rf_publics VALUES (97, 'P-A'), (98, 'P-B')",
        "INSERT INTO rf_outers VALUES (100, 50), (101, 51)",
        "INSERT INTO rf_paged_links VALUES (120, 40, 60), (121, 41, 61), (122, 40, 60)",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let router = cratestack_schema::axum::model_router(db.clone(), (), JsonCodec, CallerOne);
    Some((test_pg, db, router))
}

#[tokio::test]
async fn baseline_the_related_rows_are_hidden_from_direct_reads_and_include() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    assert_eq!(matched(&r, "/rf_hiddens").await, NONE);
    assert!(get(&r, "/rf_links/50?include=hidden").await["hidden"].is_null());
    assert_eq!(
        get(&r, "/rf_links/50?include=tags").await["tags"],
        Json::Array(vec![])
    );
}

#[tokio::test]
async fn to_one_filters_never_match_a_hidden_related_row() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    // Every operator, including the ones that would match a *different*
    // value (`ne`) or an absent one (`isNull`): a hidden row is not there.
    for query in [
        "hidden.code=CODE-A",
        "hidden.code__ne=CODE-A",
        "hidden.code__ne=NOPE",
        "hidden.code__in=CODE-A,X",
        "hidden.code__startsWith=CODE-",
        "hidden.code__contains=DE-A",
        "hidden.score__gt=15",
        "hidden.score__lte=10",
        "hidden.note__isNull=true",
        "hidden.note__isNull=false",
        // No `hidden.secret` (`@server_only`) case: whether a request may
        // name that field at all is GHSA-ch54-jqw2-vpp5's contract (a 400,
        // `tests/server_only_query.rs`), not this scope's.
        "where=hidden.code=CODE-A",
        "or=hidden.code=CODE-A|id=0",
    ] {
        assert_eq!(
            matched(&r, &format!("/rf_links?{query}")).await,
            NONE,
            "{query}"
        );
    }
    // NOT over a relation that cannot match is true for every row.
    for query in [
        "where=not(hidden.code=CODE-A)",
        "where=not(hidden.code=NOPE)",
    ] {
        assert_eq!(
            matched(&r, &format!("/rf_links?{query}")).await,
            vec![50, 51],
            "{query}"
        );
    }
    // Positive control: a readable related row still filters normally.
    assert_eq!(matched(&r, "/rf_links?public.code=P-A").await, vec![50]);
    assert_eq!(matched(&r, "/rf_links?public.code__ne=P-A").await, vec![51]);
    assert_eq!(
        matched(&r, "/rf_links?where=public.code=P-B").await,
        vec![51]
    );
}

#[tokio::test]
async fn to_many_quantifiers_only_count_readable_children() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    assert_eq!(matched(&r, "/rf_links?tags.some.label=t-a").await, NONE);
    // Over hidden-only children, `none` and `every` are vacuously true.
    assert_eq!(
        matched(&r, "/rf_links?tags.none.label=t-a").await,
        vec![50, 51]
    );
    assert_eq!(
        matched(&r, "/rf_links?tags.every.label=NOPE").await,
        vec![50, 51]
    );
}

#[tokio::test]
async fn other_policy_shapes_hide_the_related_row() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    // `@@deny` beats `@@allow`.
    assert_eq!(matched(&r, "/rf_links?denied.code=D-A").await, NONE);
    // `auth()`-scoped: 61 belongs to caller 2, 60 to the caller.
    assert_eq!(matched(&r, "/rf_links?owned.code=THEIRS").await, NONE);
    assert_eq!(matched(&r, "/rf_links?owned.code=MINE").await, vec![50]);
    // `@@soft_delete`: 91 is tombstoned, 90 is live.
    assert_eq!(matched(&r, "/rf_links?soft.code=S-GONE").await, NONE);
    assert_eq!(matched(&r, "/rf_links?soft.code=S-LIVE").await, vec![50]);
    // `@@internal("read")` is route-only: its rows stay reachable through
    // relations under their read policy.
    assert_eq!(matched(&r, "/rf_links?internal.code=I-A").await, vec![50]);
}

#[tokio::test]
async fn multi_hop_paths_apply_every_hops_scope() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    assert_eq!(
        matched(&r, "/rf_outers?link.hidden.code=CODE-A").await,
        NONE
    );
    assert_eq!(
        matched(&r, "/rf_outers?link.hidden.children.some.label=kid-a").await,
        NONE
    );
    assert_eq!(
        matched(&r, "/rf_outers?link.hidden.children.none.label=kid-a").await,
        NONE
    );
    assert_eq!(matched(&r, "/rf_outers?link.owned.code=THEIRS").await, NONE);
    assert_eq!(
        matched(&r, "/rf_outers?link.owned.code=MINE").await,
        vec![100]
    );
    assert_eq!(
        matched(&r, "/rf_outers?link.public.code=P-B").await,
        vec![101]
    );
}

#[tokio::test]
async fn relation_sorts_read_a_hidden_key_as_null() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    // Both keys hidden: every key is NULL, so both directions fall back to
    // the id tiebreak — the hidden values' order does not leak.
    for (base, key, expected) in [
        ("/rf_links", "hidden.score", vec![50, 51]),
        ("/rf_links", "hidden.code", vec![50, 51]),
        ("/rf_outers", "link.hidden.score", vec![100, 101]),
        ("/rf_outers", "link.soft.code", vec![100, 101]),
        // 51's key is hidden (caller 2's / tombstoned) so it sorts last
        // (NULLS LAST) in both directions.
        ("/rf_links", "owned.code", vec![50, 51]),
        ("/rf_links", "soft.code", vec![50, 51]),
    ] {
        assert_eq!(
            ids(&r, &format!("{base}?sort={key},id")).await,
            expected,
            "{key}"
        );
        assert_eq!(
            ids(&r, &format!("{base}?sort=-{key},id")).await,
            expected,
            "-{key}"
        );
    }
    // Positive control: readable keys still sort, one hop and two.
    assert_eq!(ids(&r, "/rf_links?sort=public.code,id").await, vec![50, 51]);
    assert_eq!(
        ids(&r, "/rf_links?sort=-public.code,id").await,
        vec![51, 50]
    );
    assert_eq!(
        ids(&r, "/rf_outers?sort=-link.public.code,id").await,
        vec![101, 100]
    );
}

/// The concrete attack: recover a hidden column one character at a time
/// through `startsWith`. It must recover nothing.
#[tokio::test]
async fn a_hidden_value_cannot_be_extracted_character_by_character() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    let mut recovered = String::new();
    'next: for _ in 0..8 {
        for ch in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-".chars() {
            let candidate = format!("{recovered}{ch}");
            let path = format!("/rf_links?id=50&hidden.code__startsWith={candidate}");
            if matched(&r, &path).await == vec![50] {
                recovered = candidate;
                continue 'next;
            }
        }
        break;
    }
    assert_eq!(recovered, "");
    // The same probe recovers a readable value, so the probe itself works.
    let path = "/rf_links?id=50&public.code__startsWith=P-";
    assert_eq!(matched(&r, path).await, vec![50]);
}

#[path = "relation_filter_policy/typed.rs"]
mod typed;

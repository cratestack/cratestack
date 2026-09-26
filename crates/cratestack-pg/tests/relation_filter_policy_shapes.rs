//! GHSA-p55v-6xv5-93p3, review follow-up: the related read scope under
//! policy shapes the first regression file does not reach — several
//! `auth()` binds in `@@allow` *and* `@@deny` at two nesting levels, a
//! related policy that itself traverses a relation (including back into
//! the outer query's own table, and through a self-relation), and whether
//! `every`/`none` over partly-hidden children can tell anything about the
//! hidden ones. Each "nothing" is paired with a positive control.

mod support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::Request;
use cratestack::include_server_schema;
use cratestack::serde_json::{self, Value as Json};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use support::pg;
use tower::util::ServiceExt;

#[path = "relation_filter_policy_shapes/typed.rs"]
mod typed;

include_server_schema!(
    "tests/fixtures/relation_filter_policy_shapes.cstack",
    db = Postgres
);

#[derive(Clone)]
struct Alice;

impl AuthProvider for Alice {
    type Error = cratestack::CratestackError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(alice()))
    }
}

fn alice() -> CratestackContext {
    CratestackContext::authenticated([
        ("id".to_owned(), Value::Int(1)),
        ("name".to_owned(), Value::String("alice".to_owned())),
    ])
}

type Router = cratestack::axum::Router;

async fn ids(router: &Router, path: &str) -> Vec<i64> {
    let mut request = Request::get(path).body(Body::empty()).unwrap();
    request
        .headers_mut()
        .insert("accept", JsonCodec::CONTENT_TYPE.parse().unwrap());
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8_lossy(&bytes).into_owned();
    assert_eq!(status, 200, "{path} -> {body}");
    let rows: Json = serde_json::from_str(&body).unwrap();
    rows.as_array()
        .unwrap_or_else(|| panic!("{path}: expected an array, got {rows}"))
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect()
}

async fn matched(router: &Router, path: &str) -> Vec<i64> {
    let mut ids = ids(router, path).await;
    ids.sort_unstable();
    ids
}

const NONE: Vec<i64> = Vec::new();

async fn setup() -> Option<(pg::TestPg, cratestack_schema::Cratestack, Router)> {
    let test_pg = pg::connect_or_skip().await?;
    for statement in [
        "DROP TABLE IF EXISTS rv_docs, rv_folders, rv_shelfs, rv_teams, rv_gateds, \
         rv_gate_links, rv_roots, rv_items, rv_members, rv_member_links, rv_boxs, rv_tokens, \
         rv_slots, rv_slot_links",
        "CREATE TABLE rv_slots (id BIGINT PRIMARY KEY, code TEXT NOT NULL)",
        "CREATE TABLE rv_slot_links (id BIGINT PRIMARY KEY, slot_id BIGINT NOT NULL)",
        // Slot 80 is listable; 81 is readable only through the detail slot.
        "INSERT INTO rv_slots VALUES (80, 'LISTED'), (81, 'DETAIL-ONLY')",
        "INSERT INTO rv_slot_links VALUES (85, 80), (86, 81)",
        "CREATE TABLE rv_docs (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, \
         label TEXT NOT NULL, code TEXT NOT NULL)",
        "CREATE TABLE rv_folders (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, \
         doc_id BIGINT NOT NULL, code TEXT NOT NULL)",
        "CREATE TABLE rv_shelfs (id BIGINT PRIMARY KEY, folder_id BIGINT NOT NULL)",
        "CREATE TABLE rv_teams (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL)",
        "CREATE TABLE rv_gateds (id BIGINT PRIMARY KEY, team_id BIGINT NOT NULL, code TEXT NOT NULL)",
        "CREATE TABLE rv_gate_links (id BIGINT PRIMARY KEY, gated_id BIGINT NOT NULL)",
        "CREATE TABLE rv_roots (id BIGINT PRIMARY KEY, flag TEXT NOT NULL, item_id BIGINT NOT NULL)",
        "CREATE TABLE rv_items (id BIGINT PRIMARY KEY, root_id BIGINT NOT NULL, code TEXT NOT NULL)",
        "CREATE TABLE rv_members (id BIGINT PRIMARY KEY, boss_id BIGINT NULL, name TEXT NOT NULL)",
        "CREATE TABLE rv_member_links (id BIGINT PRIMARY KEY, member_id BIGINT NOT NULL)",
        "CREATE TABLE rv_boxs (id BIGINT PRIMARY KEY)",
        "CREATE TABLE rv_tokens (id BIGINT PRIMARY KEY, box_id BIGINT NOT NULL, \
         label TEXT NOT NULL, shown TEXT NOT NULL)",
        // Docs: 20 readable; 21 denied (label == caller's name); 22 someone
        // else's; 23 readable.
        "INSERT INTO rv_docs VALUES (20, 1, 'ok', 'D1'), (21, 1, 'alice', 'D2'), \
         (22, 2, 'ok', 'D3'), (23, 1, 'ok', 'D0')",
        // Folders: 30/31/34 readable; 32 denied (BLOCK); 33 someone else's.
        "INSERT INTO rv_folders VALUES (30, 1, 20, 'F1'), (31, 1, 21, 'F2'), \
         (32, 1, 22, 'BLOCK'), (33, 2, 20, 'F3'), (34, 1, 23, 'F4')",
        "INSERT INTO rv_shelfs VALUES (40, 30), (41, 31), (42, 32), (43, 33), (44, 34)",
        // Team 1 is the caller's: gated 50 readable, 51 not.
        "INSERT INTO rv_teams VALUES (1, 1), (2, 2)",
        "INSERT INTO rv_gateds VALUES (50, 1, 'G-MINE'), (51, 2, 'G-THEIRS')",
        "INSERT INTO rv_gate_links VALUES (55, 50), (56, 51)",
        // Root 1 is open, root 2 closed. Item 10 belongs to closed root 2
        // (hidden) but is root 1's `item`; item 11 belongs to open root 1
        // (readable) but is root 2's `item`. A policy correlated with the
        // *outer* root would get both answers backwards.
        "INSERT INTO rv_roots VALUES (1, 'open', 10), (2, 'closed', 11)",
        "INSERT INTO rv_items VALUES (10, 2, 'I-A'), (11, 1, 'I-B')",
        // 60 root (no boss: hidden), 61 a (boss root: readable), 62 b (boss
        // a: hidden).
        "INSERT INTO rv_members VALUES (60, NULL, 'root'), (61, 60, 'a'), (62, 61, 'b')",
        "INSERT INTO rv_member_links VALUES (65, 60), (66, 61), (67, 62)",
        // Box 1: visible x + hidden y. Box 2: visible x + hidden x.
        // Box 3: hidden y only.
        "INSERT INTO rv_boxs VALUES (1), (2), (3)",
        "INSERT INTO rv_tokens VALUES (70, 1, 'x', 'yes'), (71, 1, 'y', 'no'), \
         (72, 2, 'x', 'yes'), (73, 2, 'x', 'no'), (74, 3, 'y', 'no')",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let router = cratestack_schema::axum::model_router(db.clone(), (), JsonCodec, Alice);
    Some((test_pg, db, router))
}

#[tokio::test]
async fn allow_and_deny_with_auth_binds_apply_at_every_hop() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    // Baselines: the direct reads agree with the policies.
    assert_eq!(matched(&r, "/rv_docs").await, vec![20, 23]);
    assert_eq!(matched(&r, "/rv_folders").await, vec![30, 31, 34]);

    assert_eq!(matched(&r, "/rv_shelfs?folder.code=F1").await, vec![40]);
    assert_eq!(matched(&r, "/rv_shelfs?folder.code=BLOCK").await, NONE);
    assert_eq!(matched(&r, "/rv_shelfs?folder.code=F3").await, NONE);
    // D1 is readable, but shelf 43's folder (33) is not.
    assert_eq!(matched(&r, "/rv_shelfs?folder.doc.code=D1").await, vec![40]);
    // Denied by `label == auth().name` at the second hop.
    assert_eq!(matched(&r, "/rv_shelfs?folder.doc.code=D2").await, NONE);
    assert_eq!(matched(&r, "/rv_shelfs?folder.doc.label=alice").await, NONE);
    // Someone else's doc, behind a denied folder.
    assert_eq!(matched(&r, "/rv_shelfs?folder.doc.code=D3").await, NONE);
    assert_eq!(
        matched(&r, "/rv_shelfs?or=folder.doc.code=D2|folder.doc.code=D1").await,
        vec![40]
    );
    assert_eq!(
        matched(&r, "/rv_shelfs?where=not(folder.doc.code=D1)").await,
        vec![41, 42, 43, 44]
    );
    // Sort: only 40 (D1) and 44 (D0) have a readable key; the rest are
    // NULL (NULLS LAST) in both directions.
    assert_eq!(
        ids(&r, "/rv_shelfs?sort=folder.doc.code,id").await,
        vec![44, 40, 41, 42, 43]
    );
    assert_eq!(
        ids(&r, "/rv_shelfs?sort=-folder.doc.code,id").await,
        vec![40, 44, 41, 42, 43]
    );
}

#[tokio::test]
async fn a_related_policy_that_traverses_a_relation_still_applies() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    assert_eq!(matched(&r, "/rv_gateds").await, vec![50]);
    assert_eq!(
        matched(&r, "/rv_gate_links?gated.code=G-MINE").await,
        vec![55]
    );
    assert_eq!(
        matched(&r, "/rv_gate_links?gated.code=G-THEIRS").await,
        NONE
    );
    assert_eq!(
        ids(&r, "/rv_gate_links?sort=-gated.code,id").await,
        vec![55, 56]
    );
}

#[tokio::test]
async fn a_related_policy_into_the_outer_table_correlates_with_the_related_row() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    assert_eq!(matched(&r, "/rv_items").await, vec![11]);
    // Item 10 is hidden (its own root is closed) although the outer row
    // pointing at it (root 1) is open.
    assert_eq!(matched(&r, "/rv_roots?item.code=I-A").await, NONE);
    // Item 11 is readable (its own root is open) although the outer row
    // pointing at it (root 2) is closed.
    assert_eq!(matched(&r, "/rv_roots?item.code=I-B").await, vec![2]);
    assert_eq!(matched(&r, "/rv_roots?item.root.flag=open").await, vec![2]);
}

#[tokio::test]
async fn a_related_policy_through_a_self_relation_correlates() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    assert_eq!(matched(&r, "/rv_members").await, vec![61]);
    assert_eq!(
        matched(&r, "/rv_member_links?member.name=a").await,
        vec![66]
    );
    assert_eq!(matched(&r, "/rv_member_links?member.name=b").await, NONE);
    assert_eq!(matched(&r, "/rv_member_links?member.name=root").await, NONE);
    // Member 61 is readable and its boss is 60 ("root") — but 60 itself is
    // hidden (it has no boss), so the second hop's scope hides it even
    // though 61's own policy reads it.
    assert_eq!(
        matched(&r, "/rv_member_links?member.boss.name=root").await,
        NONE
    );
    // 62's boss (61) is readable but 62 itself is not.
    assert_eq!(
        matched(&r, "/rv_member_links?member.boss.name=a").await,
        NONE
    );
}

#[tokio::test]
async fn quantifiers_do_not_depend_on_what_hidden_children_hold() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    // Boxes 1 and 2 differ only in a hidden child's label; box 3 has only a
    // hidden child. No quantifier may tell them apart by those labels.
    assert_eq!(
        matched(&r, "/rv_boxs?tokens.every.label=x").await,
        vec![1, 2, 3]
    );
    assert_eq!(
        matched(&r, "/rv_boxs?tokens.none.label=y").await,
        vec![1, 2, 3]
    );
    assert_eq!(matched(&r, "/rv_boxs?tokens.some.label=y").await, NONE);
    assert_eq!(
        matched(&r, "/rv_boxs?tokens.some.label=x").await,
        vec![1, 2]
    );
    assert_eq!(matched(&r, "/rv_boxs?tokens.none.label=x").await, vec![3]);
    assert_eq!(
        matched(&r, "/rv_boxs?tokens.every.shown=yes").await,
        vec![1, 2, 3]
    );
}

//! Self-relations (`RfPerson.manager -> RfPerson`). A relation subquery
//! into the parent's own table used to render as
//! `FROM rf_persons WHERE rf_persons.id = rf_persons.manager_id`, where both
//! sides bind to the *inner* row: the filter was not correlated with the
//! outer row at all (it matched every row, or none, depending on whether
//! some row happened to be its own manager). The same shape made a policy
//! that traverses a self-relation (`RfMember`'s `boss.name == "root"`)
//! uncorrelated. These tests pin the correct semantics, including the
//! related read scope (hidden `secret` persons read as nonexistent).

mod support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::Request;
use cratestack::include_server_schema;
use cratestack::serde_json::{self, Value as Json};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use support::pg;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/relation_self.cstack", db = Postgres);

#[derive(Clone)]
struct Caller;

impl AuthProvider for Caller {
    type Error = cratestack::CratestackError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

async fn ids(router: &cratestack::axum::Router, path: &str) -> Vec<i64> {
    let mut request = Request::get(path).body(Body::empty()).unwrap();
    request
        .headers_mut()
        .insert("accept", JsonCodec::CONTENT_TYPE.parse().unwrap());
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200, "{path}");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let rows: Json = serde_json::from_slice(&bytes).unwrap();
    rows.as_array()
        .unwrap_or_else(|| panic!("{path}: expected an array, got {rows}"))
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect()
}

/// The matched ids in ascending order (a list without `sort` has none).
async fn matched(router: &cratestack::axum::Router, path: &str) -> Vec<i64> {
    let mut ids = ids(router, path).await;
    ids.sort_unstable();
    ids
}

async fn setup() -> Option<(support::pg::TestPg, cratestack::axum::Router)> {
    let test_pg = pg::connect_or_skip().await?;
    for statement in [
        "DROP TABLE IF EXISTS rf_persons, rf_members",
        "CREATE TABLE rf_persons (id BIGINT PRIMARY KEY, manager_id BIGINT NULL, \
         name TEXT NOT NULL, tier TEXT NOT NULL)",
        "CREATE TABLE rf_members (id BIGINT PRIMARY KEY, boss_id BIGINT NULL, name TEXT NOT NULL)",
        // 1 boss <- 2 mid <- 3 low; 4 is its own manager; 5 is hidden and
        // manages 6.
        "INSERT INTO rf_persons VALUES (1, NULL, 'boss', 'public'), (2, 1, 'mid', 'public'), \
         (3, 2, 'low', 'public'), (4, 4, 'self', 'public'), (5, NULL, 'spy', 'secret'), \
         (6, 5, 'mole', 'public')",
        // 10 root <- 11 <- 12; 13 is its own boss and is named root.
        "INSERT INTO rf_members VALUES (10, NULL, 'root'), (11, 10, 'a'), (12, 11, 'b'), \
         (13, 13, 'root')",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let router = cratestack_schema::axum::model_router(db, (), JsonCodec, Caller);
    Some((test_pg, router))
}

#[tokio::test]
async fn self_relation_filters_correlate_with_the_outer_row() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, r)) = setup().await else {
        return;
    };
    assert_eq!(matched(&r, "/rf_persons?manager.name=boss").await, vec![2]);
    assert_eq!(matched(&r, "/rf_persons?manager.name=mid").await, vec![3]);
    assert_eq!(matched(&r, "/rf_persons?manager.name=self").await, vec![4]);
    assert_eq!(
        matched(&r, "/rf_persons?manager.manager.name=boss").await,
        vec![3]
    );
    assert_eq!(
        matched(&r, "/rf_persons?reports.some.name=mid").await,
        vec![1]
    );
    assert_eq!(
        matched(&r, "/rf_persons?reports.none.name=mid").await,
        vec![2, 3, 4, 6]
    );
    // The hidden manager (5, `secret`) of row 6 reads as nonexistent.
    assert_eq!(
        matched(&r, "/rf_persons?manager.name=spy").await,
        Vec::<i64>::new()
    );
    assert_eq!(
        matched(&r, "/rf_persons?manager.name__ne=zzz").await,
        vec![2, 3, 4]
    );
}

#[tokio::test]
async fn self_relation_sort_reads_the_outer_rows_related_value() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, r)) = setup().await else {
        return;
    };
    // NULLS LAST: row 1 has no manager, row 6's manager is hidden.
    assert_eq!(
        ids(&r, "/rf_persons?sort=manager.name,id").await,
        vec![2, 3, 4, 1, 6]
    );
    assert_eq!(
        ids(&r, "/rf_persons?sort=-manager.name,id").await,
        vec![4, 3, 2, 1, 6]
    );
}

#[tokio::test]
async fn a_policy_traversing_a_self_relation_is_correlated() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, r)) = setup().await else {
        return;
    };
    // Readable iff *this row's* boss is named root: 11 (boss 10) and 13
    // (its own boss). Not 10 (no boss) nor 12 (boss 11 is named "a").
    assert_eq!(matched(&r, "/rf_members").await, vec![11, 13]);
}

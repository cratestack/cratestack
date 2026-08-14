//! Live end-to-end verification of the pgvector distance-operator query
//! builder (cratestack#163): actually runs `.order_by_distance(...)` /
//! `.distance_to(...)` through the generated `Document` model against a
//! real Postgres with the `vector` extension, and checks the returned
//! row order is the true nearest-neighbor order for each of the three
//! metrics — not just that the emitted SQL string looks right (that's
//! covered separately, with no DB required, by
//! `cratestack-sqlx::tests_pgvector`).
//!
//! **Requires a pgvector-enabled Postgres.** The repo's default
//! `compose.yml` container (`postgres:18`, no `vector` extension) is
//! NOT sufficient — this test needs `CRATESTACK_TEST_DATABASE_URL`
//! pointed at a Postgres built from the `pgvector/pgvector:pg17` image
//! (`CREATE EXTENSION IF NOT EXISTS vector` fails hard otherwise, it
//! does not skip). `testcontainers`' generic Postgres image is
//! similarly unsuitable, so this file only supports the explicit-URL
//! backend from `support::pg` — the testcontainers/skip branches never
//! spin up a pgvector-capable image, so this test's `reset_schema`
//! would hard-fail on `CREATE EXTENSION vector` there too; unlike every
//! other `CRATESTACK_TEST_DATABASE_URL`-gated test in this crate, this
//! one is not part of `just test-pg`/`just test-pg-tc`'s normal green
//! path and must be run explicitly, e.g.:
//!
//! ```sh
//! docker run -d --name cratestack-pgvector -p 55433:5432 \
//!   -e POSTGRES_USER=cratestack -e POSTGRES_PASSWORD=cratestack \
//!   -e POSTGRES_DB=cratestack_pgvector pgvector/pgvector:pg17
//! CRATESTACK_TEST_DATABASE_URL=postgres://cratestack:cratestack@localhost:55433/cratestack_pgvector \
//!   cargo test -p cratestack-pg --features pgvector --test pgvector_distance_query
//! docker rm -f cratestack-pgvector
//! ```
//!
//! Gated `required-features = ["pgvector"]` in `Cargo.toml`, same as
//! `pgvector_feature_forwarding.rs`.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CratestackContext, Value, VectorMetric};
use support::pg;

include_server_schema!(
    "tests/fixtures/pgvector_distance_query.cstack",
    db = Postgres
);

use cratestack_schema::document;

fn ctx() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS documents")
        .execute(pool)
        .await
        .expect("drop documents");
    query("CREATE EXTENSION IF NOT EXISTS vector")
        .execute(pool)
        .await
        .expect(
            "CREATE EXTENSION vector — this test needs a Postgres built from the \
             pgvector/pgvector image, not the repo's default compose.yml Postgres; \
             see this file's module docs for how to point CRATESTACK_TEST_DATABASE_URL \
             at one",
        );
    query(
        "CREATE TABLE documents (
            id BIGINT PRIMARY KEY,
            label TEXT NOT NULL,
            embedding vector(3) NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create documents table");
}

/// Three rows, deliberately chosen so L2 / cosine / inner-product each
/// produce a *different* nearest-first order relative to the query
/// vector `[1.0, 0.0, 0.0]` below — proof the metric argument actually
/// changes which operator runs, not just that some operator runs.
/// Hand-computed distances (query = `[1, 0, 0]`):
///
/// | doc  | vector            | L2     | cosine  | inner product |
/// |------|--------------------|--------|---------|---------------|
/// | near | `[1.1, 0.1, 0.0]`  | 0.1414 | 0.00407 | -1.1          |
/// | mid  | `[5.0, 0.5, 0.0]`  | 4.031  | 0.00497 | -5.0          |
/// | far  | `[10.0, 0.0, 0.0]` | 9.0    | 0.0     | -10.0         |
///
/// (cosine distance = 1 - cosine similarity; inner product is
/// pgvector's *negative* dot product, so more negative = more similar,
/// ascending order still means "nearest first".)
async fn seed(pool: &cratestack::sqlx::PgPool) {
    let rows: &[(i64, &str, Vec<f32>)] = &[
        (1, "near", vec![1.1, 0.1, 0.0]),
        (2, "mid", vec![5.0, 0.5, 0.0]),
        (3, "far", vec![10.0, 0.0, 0.0]),
    ];
    for (id, label, embedding) in rows {
        query("INSERT INTO documents (id, label, embedding) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(*label)
            .bind(cratestack::pgvector::Vector::from(embedding.clone()))
            .execute(pool)
            .await
            .expect("seed document");
    }
}

fn query_vector() -> Vec<f32> {
    vec![1.0, 0.0, 0.0]
}

#[tokio::test]
async fn order_by_distance_l2_returns_true_nearest_neighbor_order() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    // AC #2: no `@@index(...)` is declared on `embedding` anywhere in
    // the fixture — this is a plain sequential scan, proving distance
    // ordering does not depend on an index existing.
    let results = cool
        .document()
        .find_many()
        .order_by(document::embedding().order_by_distance(VectorMetric::L2, query_vector()))
        .run(&ctx())
        .await
        .expect("L2 distance order-by must run against real Postgres");

    let labels: Vec<&str> = results.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["near", "mid", "far"],
        "L2 (`<->`) must return the true Euclidean nearest-neighbor order",
    );
}

#[tokio::test]
async fn order_by_distance_cosine_returns_true_nearest_neighbor_order() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let results = cool
        .document()
        .find_many()
        .order_by(document::embedding().order_by_distance(VectorMetric::Cosine, query_vector()))
        .run(&ctx())
        .await
        .expect("cosine distance order-by must run against real Postgres");

    let labels: Vec<&str> = results.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["far", "near", "mid"],
        "cosine (`<=>`) ranks purely by direction — `far` is exactly \
         co-directional with the query vector despite being farthest \
         in raw Euclidean terms",
    );
}

#[tokio::test]
async fn order_by_distance_inner_product_returns_true_nearest_neighbor_order() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let results = cool
        .document()
        .find_many()
        .order_by(
            document::embedding().order_by_distance(VectorMetric::InnerProduct, query_vector()),
        )
        .run(&ctx())
        .await
        .expect("inner-product distance order-by must run against real Postgres");

    let labels: Vec<&str> = results.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["far", "mid", "near"],
        "inner product (`<#>`) ranks by raw (negative) dot product, the \
         inverse of the L2 order for these vectors",
    );
}

#[tokio::test]
async fn distance_threshold_filter_excludes_rows_beyond_the_cutoff() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let results = cool
        .document()
        .find_many()
        .where_expr(
            document::embedding()
                .distance_to(VectorMetric::L2, query_vector())
                .lte(1.0_f64),
        )
        .order_by(document::embedding().order_by_distance(VectorMetric::L2, query_vector()))
        .run(&ctx())
        .await
        .expect("distance threshold filter must run against real Postgres");

    let labels: Vec<&str> = results.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["near"],
        "only `near` (L2 distance ~0.1414) is within the 1.0 cutoff — \
         `mid` (~4.03) and `far` (9.0) must be excluded",
    );
}

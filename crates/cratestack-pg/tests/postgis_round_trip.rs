//! Live round-trip for a `Geography` column through the **generated
//! model** (cratestack#842).
//!
//! This exists because nothing else covers the decode half. The sibling
//! `postgis_feature_forwarding` test builds the struct by hand, so it
//! only proves the generated surface *compiles*; `tier7` drives spatial
//! filters but its column is declared `Bytes` and never decoded as a
//! geography. That left `cratestack_sqlx::Ewkb` — the type-compatibility
//! shim the generated row decoder goes through — verified by nothing at
//! runtime.
//!
//! The shim is not incidental. Binding is easy (PostGIS registers an
//! implicit `bytea` cast, so a plain byte bind works), but on the way
//! *out* the column's type OID is `geography`'s, and sqlx type-checks
//! that OID against the Rust type before handing over the payload. A
//! bare `Vec<u8>` fails there with "mismatched types". `Ewkb` declares
//! itself compatible with both spatial type names; if that declaration
//! is ever wrong, this test is what catches it.
//!
//! Requires a PostGIS-enabled Postgres. Point
//! `CRATESTACK_TEST_DATABASE_URL` at one (e.g. `postgis/postgis:16-3.4`);
//! otherwise `CREATE EXTENSION postgis` fails and the test skips, same
//! as `tier7`.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CratestackContext, Value};
use support::pg;

include_server_schema!(
    "tests/fixtures/postgis_feature_forwarding.cstack",
    db = Postgres
);

async fn ensure_postgis_or_skip(pool: &cratestack::sqlx::PgPool) -> bool {
    match query("CREATE EXTENSION IF NOT EXISTS postgis;")
        .execute(pool)
        .await
    {
        Ok(_) => true,
        Err(_) => {
            eprintln!(
                "skipping PostGIS round-trip: `CREATE EXTENSION postgis` failed (image \
                 likely lacks the spatial extension)"
            );
            false
        }
    }
}

/// Builds the table with the *emitter's own* column type — a real
/// `geography(Polygon,4326)`, not a `BYTEA` stand-in. Decoding a bytea
/// column would pass trivially and prove nothing about `Ewkb`.
async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS coverage_areas")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE coverage_areas (
            id BIGINT PRIMARY KEY,
            label TEXT NOT NULL,
            service_area geography(Polygon,4326) NOT NULL,
            pickup_point geography(Point,4326)
        )",
    )
    .execute(pool)
    .await
    .expect("create table");
    // Static SQL — no interpolation, so no `AssertSqlSafe` needed.
    query(
        "INSERT INTO coverage_areas (id, label, service_area)
         VALUES (1, 'central', ST_GeogFromText('SRID=4326;POLYGON((0 0,2 0,2 2,0 2,0 0))'))",
    )
    .execute(pool)
    .await
    .expect("seed zone");
}

fn operator() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("postgis-round-trip-001")
}

/// The decisive assertion: a `geography` column decodes through the
/// generated `FromRow` into the model's `Vec<u8>` field, and the bytes
/// are the EWKB PostGIS itself reports.
#[tokio::test]
async fn geography_column_decodes_through_the_generated_model() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    if !ensure_postgis_or_skip(pool).await {
        return;
    }
    reset_schema(pool).await;

    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let zone = db
        .coverage_area()
        .bind(operator())
        .find_unique(1)
        .run()
        .await
        .expect("decoding a geography column must not fail")
        .expect("seeded row exists");

    assert_eq!(zone.label, "central");
    assert!(
        !zone.service_area.is_empty(),
        "a NOT NULL geography must decode to non-empty EWKB"
    );

    // Compare against PostGIS's own EWKB for the same row, so this
    // asserts the real wire format rather than "some bytes came back".
    let expected: Vec<u8> = query("SELECT ST_AsEWKB(service_area::geometry) FROM coverage_areas")
        .fetch_one(pool)
        .await
        .expect("fetch expected ewkb")
        .get(0);
    assert_eq!(
        zone.service_area, expected,
        "decoded bytes must be the EWKB PostGIS reports for the same column"
    );

    assert_eq!(
        zone.pickup_point, None,
        "a NULL geography must decode as None, not empty bytes"
    );
}

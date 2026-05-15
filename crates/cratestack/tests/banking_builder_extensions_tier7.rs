//! End-to-end test for PostGIS spatial filters
//! (`.covers_geography(...)` / `.dwithin_geography(...)`).
//!
//! Requires a Postgres with the `postgis` extension available. If the
//! `CREATE EXTENSION` call fails (e.g. the default testcontainers
//! image lacks PostGIS), each test skips cleanly so the suite stays
//! green in environments without spatial extensions installed. Point
//! `CRATESTACK_TEST_DATABASE_URL` at a PostGIS-enabled Postgres
//! (e.g. `postgis/postgis:16-3.4`) to exercise the integration.
//!
//! Render-level correctness is also covered by unit tests in
//! `cratestack-sqlx`, so this file is the live-engine sanity check.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CoolContext, Value, point};
use support::pg;

include_server_schema!("tests/fixtures/builder_extensions_tier7.cstack", db = Postgres);

async fn ensure_postgis_or_skip(pool: &cratestack::sqlx::PgPool) -> bool {
    // `CREATE EXTENSION IF NOT EXISTS postgis;` succeeds when the
    // image carries the postgis package; otherwise the call errors
    // and we treat the test as skipped.
    match query("CREATE EXTENSION IF NOT EXISTS postgis;")
        .execute(pool)
        .await
    {
        Ok(_) => true,
        Err(_) => {
            eprintln!(
                "skipping PostGIS test: `CREATE EXTENSION postgis` failed (image likely \
                 lacks the spatial extension)"
            );
            false
        }
    }
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS delivery_zones")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE delivery_zones (
            id BIGINT PRIMARY KEY,
            label TEXT NOT NULL,
            service_area geography(Polygon, 4326) NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create delivery_zones");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("tier7-001")
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    // Two zones, each a small polygon. Bay Area roughly:
    //   zone A → bounding box around San Francisco (37.70..37.82, -122.52..-122.36)
    //   zone B → bounding box around Oakland          (37.72..37.86, -122.30..-122.16)
    query(
        "INSERT INTO delivery_zones (id, label, service_area) VALUES
            (1, 'sf', ST_GeogFromText('SRID=4326;POLYGON((-122.52 37.70, -122.36 37.70, -122.36 37.82, -122.52 37.82, -122.52 37.70))')),
            (2, 'oakland', ST_GeogFromText('SRID=4326;POLYGON((-122.30 37.72, -122.16 37.72, -122.16 37.86, -122.30 37.86, -122.30 37.72))'))",
    )
    .execute(pool)
    .await
    .expect("seed zones");
}

#[tokio::test]
async fn covers_geography_matches_zone_containing_the_point() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    if !ensure_postgis_or_skip(pool).await {
        return;
    }
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // SF Embarcadero: lng=-122.3971, lat=37.7955 — inside the SF zone.
    use cratestack_schema::delivery_zone;
    let count: i64 = cool
        .delivery_zone()
        .bind(ctx)
        .aggregate()
        .count()
        .where_expr(delivery_zone::serviceArea().covers_geography(point(-122.3971, 37.7955)))
        .run()
        .await
        .expect("spatial filter runs");
    assert_eq!(count, 1, "exactly the SF zone covers Embarcadero");

    // Verify which zone matched.
    let row = query(
        "SELECT label FROM delivery_zones WHERE ST_Covers(service_area::geography, \
         ST_MakePoint($1, $2)::geography)",
    )
    .bind(-122.3971)
    .bind(37.7955)
    .fetch_one(pool)
    .await
    .unwrap();
    let label: String = row.get(0);
    assert_eq!(label, "sf");
}

#[tokio::test]
async fn covers_geography_misses_point_outside_all_zones() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    if !ensure_postgis_or_skip(pool).await {
        return;
    }
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    use cratestack_schema::delivery_zone;
    // Sacramento — well outside both Bay Area zones.
    let count: i64 = cool
        .delivery_zone()
        .bind(ctx)
        .aggregate()
        .count()
        .where_expr(delivery_zone::serviceArea().covers_geography(point(-121.4944, 38.5816)))
        .run()
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn dwithin_geography_matches_zones_within_radius() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    if !ensure_postgis_or_skip(pool).await {
        return;
    }
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    use cratestack_schema::delivery_zone;

    // Bay Bridge midspan, ~37.8189, -122.3631 — straddles both zone
    // boundaries within a few km radius. 25km radius should pick up
    // both zones; the point isn't inside either polygon directly.
    let count: i64 = cool
        .delivery_zone()
        .bind(ctx)
        .aggregate()
        .count()
        .where_expr(
            delivery_zone::serviceArea().dwithin_geography(point(-122.3631, 37.8189), 25_000.0),
        )
        .run()
        .await
        .unwrap();
    assert_eq!(count, 2, "both Bay Area zones fall within 25km");
}

#[tokio::test]
async fn covers_geography_preview_emits_st_covers_with_three_binds_per_call() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    use cratestack_schema::delivery_zone;
    let preview = cool
        .delivery_zone()
        .find_many()
        .where_expr(delivery_zone::serviceArea().covers_geography(point(-122.4, 37.8)))
        .preview_sql();
    assert!(
        preview.contains("ST_Covers(service_area::geography, ST_MakePoint($1, $2)::geography)"),
        "got: {preview}",
    );
}

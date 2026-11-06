//! A string enum converted to ordinals, and back, on a Postgres table that
//! holds rows (`docs/design/enum-ordinals.md` §4, §3.4). The SQL comes from
//! the real `diff` + emitter; this proves it runs and moves the data: every
//! stored name becomes its number (and back), the default follows, NULL
//! stays NULL, and the new `CHECK` refuses a number with no variant.

use cratestack::sqlx::{Row as _, query};
use cratestack::{Migration, apply_pending};
use cratestack_migrate::diff;
use cratestack_migrate::emit::postgres;
use cratestack_parser::parse_schema;

mod support;
use support::pg;

fn schema(variants: &str) -> cratestack_core::Schema {
    parse_schema(&format!(
        "datasource db {{\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}}\n\n\
         enum ProbeKind {{\n{variants}\n}}\n\n\
         model ConversionProbe {{\n  id Int @id\n  kind ProbeKind @default(Device)\n  \
         fallback ProbeKind?\n}}\n"
    ))
    .expect("schema parses")
}

async fn apply(pool: &cratestack::sqlx::PgPool, id: &str, up: String) {
    apply_pending(
        pool,
        &[Migration {
            id: id.to_owned(),
            description: "enum ordinal conversion probe".to_owned(),
            up_pre: None,
            up,
            down: None,
        }],
    )
    .await
    .unwrap_or_else(|error| panic!("{id} must apply: {error:?}"));
}

async fn rows(pool: &cratestack::sqlx::PgPool, cast: &str) -> Vec<(i64, String, Option<String>)> {
    // AssertSqlSafe: `cast` is one of two literals in this file.
    let sql =
        format!("SELECT id, kind::{cast}, fallback::{cast} FROM conversion_probes ORDER BY id");
    query(cratestack::sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .expect("select")
        .iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect()
}

#[tokio::test]
async fn a_populated_string_enum_converts_to_ordinals_and_back() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    query("DROP TABLE IF EXISTS cratestack_migrations, conversion_probes")
        .execute(pool)
        .await
        .expect("drop");

    let empty = parse_schema("").expect("empty schema parses");
    let names = schema("  Device\n  Person");
    let ordinals = schema("  Device = 1\n  Person = 2");

    apply(
        pool,
        "20261026000001_names",
        postgres::emit(&diff(&empty, &names).expect("diff")).up,
    )
    .await;
    query("INSERT INTO conversion_probes (id, kind, fallback) VALUES (1, 'Person', 'Device'), (2, 'Device', NULL)")
        .execute(pool)
        .await
        .expect("seed");

    let to_ordinals = postgres::emit(&diff(&names, &ordinals).expect("diff"));
    assert!(
        to_ordinals.has_lossy,
        "a type change is lossy: {}",
        to_ordinals.up
    );
    apply(pool, "20261026000002_ordinals", to_ordinals.up).await;
    assert_eq!(
        rows(pool, "text").await,
        [
            (1, "2".to_owned(), Some("1".to_owned())),
            (2, "1".to_owned(), None),
        ]
    );
    let kind_type: String = query(
        "SELECT data_type FROM information_schema.columns \
         WHERE table_name = 'conversion_probes' AND column_name = 'kind'",
    )
    .fetch_one(pool)
    .await
    .expect("column type")
    .get(0);
    assert_eq!(kind_type, "smallint");

    query("INSERT INTO conversion_probes (id) VALUES (3)")
        .execute(pool)
        .await
        .expect("the default is now the ordinal");
    assert_eq!(rows(pool, "text").await[2].1, "1");
    let refused = query("INSERT INTO conversion_probes (id, kind) VALUES (4, 3)")
        .execute(pool)
        .await;
    assert!(
        refused.is_err(),
        "3 is no variant, the CHECK must refuse it"
    );

    let back = postgres::emit(&diff(&ordinals, &names).expect("diff"));
    apply(pool, "20261026000003_names_again", back.up).await;
    assert_eq!(
        rows(pool, "text").await,
        [
            (1, "Person".to_owned(), Some("Device".to_owned())),
            (2, "Device".to_owned(), None),
            (3, "Device".to_owned(), None),
        ]
    );
}

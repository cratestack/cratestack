//! cratestack#1209: function-shaped `@default(...)` values against a real
//! Postgres, through the emitter's own DDL (`fixtures/generated_defaults.cstack`).
//!
//! - cratestack's `uuid()`, `cuid()` and `now()` are filled by the runtime
//!   on create, and the DDL carries `gen_random_uuid()` / `now()` for
//!   writes that bypass cratestack (`cuid()` has no database equivalent).
//! - A function the datasource registers is written verbatim; Postgres
//!   fills it, and refuses the migration when the function does not exist.

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CratestackContext, Value};
use cratestack_core::error::parse_cuid;
use cratestack_migrate::diff;
use cratestack_migrate::emit::postgres;
use cratestack_parser::parse_schema;

include_server_schema!("tests/fixtures/generated_defaults.cstack", db = Postgres);

mod support;
use support::pg;

const SCHEMA_SRC: &str = include_str!("fixtures/generated_defaults.cstack");

const CREATE_LABEL_FUNCTION: &str = "CREATE OR REPLACE FUNCTION cs1209_ticket_label() \
                                     RETURNS text LANGUAGE sql AS $$ SELECT 'from-db'::text $$";

fn ctx() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

fn emitted_up() -> String {
    let empty = parse_schema("").expect("empty schema parses");
    let next = parse_schema(SCHEMA_SRC).expect("fixture parses");
    postgres::emit(&diff(&empty, &next).expect("diff should succeed")).up
}

async fn reset(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_migrations, tickets")
        .execute(pool)
        .await
        .expect("drop tables");
    query("DROP FUNCTION IF EXISTS cs1209_ticket_label()")
        .execute(pool)
        .await
        .expect("drop function");
}

async fn apply_emitted_migration(
    pool: &cratestack::sqlx::PgPool,
) -> Result<Vec<String>, cratestack::CratestackError> {
    cratestack::apply_pending(
        pool,
        &[cratestack::Migration {
            id: "20261023000000_generated_defaults".to_owned(),
            description: "function-shaped defaults (cratestack#1209)".to_owned(),
            up_pre: None,
            up: emitted_up(),
            down: None,
        }],
    )
    .await
}

#[test]
fn emitted_ddl_names_only_functions_postgres_has() {
    let up = emitted_up();
    assert!(up.contains("DEFAULT gen_random_uuid()"), "{up}");
    assert!(up.contains("DEFAULT now()"), "{up}");
    assert!(up.contains("DEFAULT cs1209_ticket_label()"), "{up}");
    assert!(!up.contains("uuid()\n") && !up.contains("cuid()"), "{up}");
}

#[tokio::test]
async fn cratestack_and_raw_sql_writes_both_get_their_defaults() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    query(CREATE_LABEL_FUNCTION)
        .execute(pool)
        .await
        .expect("create function");
    apply_emitted_migration(pool)
        .await
        .expect("emitter-generated DDL must apply against real Postgres");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let mut created = Vec::new();
    for title in ["first", "second"] {
        let before = chrono::Utc::now();
        let ticket = cool
            .ticket()
            .create(cratestack_schema::CreateTicketInput {
                title: title.to_owned(),
            })
            .run(&ctx())
            .await
            .expect("create through cratestack");
        assert_eq!(ticket.id.get_version_num(), 4);
        assert!(parse_cuid(&ticket.code).is_ok(), "{}", ticket.code);
        uuid::Uuid::parse_str(&ticket.reference).expect("reference is a uuid");
        assert!(ticket.createdAt >= before - chrono::Duration::seconds(5));
        assert_eq!(ticket.label, "from-db", "the registered function ran");
        created.push(ticket);
    }
    assert_ne!(created[0].id, created[1].id);
    assert_ne!(created[0].code, created[1].code);

    // A writer that bypasses cratestack gets the database defaults; only
    // `cuid()`, which Postgres has no equivalent for, must be supplied.
    let row = query(
        "INSERT INTO tickets (code, title) VALUES ('craw0000000000000000000', 'raw') \
         RETURNING id, reference, created_at, label",
    )
    .fetch_one(pool)
    .await
    .expect("raw insert");
    let id: uuid::Uuid = row.get("id");
    assert_eq!(id.get_version_num(), 4);
    uuid::Uuid::parse_str(row.get::<&str, _>("reference")).expect("text uuid");
    let _: chrono::DateTime<chrono::Utc> = row.get("created_at");
    assert_eq!(row.get::<&str, _>("label"), "from-db");
}

/// Registering a function asserts it exists; Postgres checks the claim
/// when the migration runs.
#[tokio::test]
async fn a_registered_function_that_does_not_exist_fails_the_migration() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    let error = apply_emitted_migration(pool)
        .await
        .expect_err("the function was never created");
    let message = format!("{error:?}");
    assert!(
        message.contains("cs1209_ticket_label() does not exist"),
        "{message}"
    );
}

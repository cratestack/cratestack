use cratestack_sqlx::{Migration, apply_pending};
use tempfile::TempDir;

use super::{WIDGET_SCHEMA, block_on, connect, exec, isolated_test_db, write_schema};
use crate::cli_types::BaselineBackendArg;
use crate::migrate::handle_baseline;

/// Design doc §8 case 4 (§5.3 option (b) — snapshot + synthetic
/// runner row): after baseline, `apply_pending()` run with only
/// post-baseline migrations doesn't attempt any pre-baseline DDL
/// (proven by the pre-existing table not being re-created and the
/// post-baseline migration applying cleanly), and the synthetic
/// baseline row is visible in `cratestack_migrations` — checked via a
/// direct row lookup, since `status()` only ever reports on the
/// migrations passed to it, and this scenario deliberately passes
/// only the post-baseline one.
#[test]
fn apply_pending_after_baseline_skips_pre_baseline_ddl_and_row_is_recorded() {
    let Some(isolated_url) = isolated_test_db("apply_pending") else {
        return;
    };

    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, WIDGET_SCHEMA);
    let out = dir.path().join("migrations");

    block_on(async {
        let pool = connect(&isolated_url).await;
        exec(
            &pool,
            "CREATE TABLE widgets (id BIGINT NOT NULL PRIMARY KEY, name TEXT NOT NULL); \
             CREATE UNIQUE INDEX widgets_name_key ON widgets (name);",
        )
        .await;
    });

    handle_baseline(
        schema,
        isolated_url.clone(),
        out,
        BaselineBackendArg::Postgres,
        false,
    )
    .expect("clean baseline should succeed");

    block_on(async {
        let pool = connect(&isolated_url).await;

        let baseline_rows: Vec<(String, String)> = sqlx_core::query_as::query_as(
            "SELECT id, description FROM cratestack_migrations WHERE id LIKE '%_baseline'",
        )
        .fetch_all(&pool)
        .await
        .expect("query cratestack_migrations");
        assert_eq!(
            baseline_rows.len(),
            1,
            "exactly one synthetic baseline row should be recorded"
        );
        assert!(baseline_rows[0].1.contains("adopted 1 existing table"));

        // Only a post-baseline migration in the list — proves
        // `apply_pending` neither tries (nor needs) to recreate
        // `widgets`, which baseline already accounted for.
        let post_baseline = Migration {
            id: "20990101000000_add_gadgets".to_owned(),
            description: "add gadgets table".to_owned(),
            up_pre: None,
            up: "CREATE TABLE gadgets (id BIGINT NOT NULL PRIMARY KEY)".to_owned(),
            down: None,
        };
        let applied = apply_pending(&pool, std::slice::from_ref(&post_baseline))
            .await
            .expect("apply_pending should only touch the post-baseline migration");
        assert_eq!(applied, vec![post_baseline.id.clone()]);

        let gadgets_exists: bool = sqlx_core::query_scalar::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_name = 'gadgets' AND table_schema = current_schema())",
        )
        .fetch_one(&pool)
        .await
        .expect("check gadgets table");
        assert!(gadgets_exists, "post-baseline migration should have run");
    });
}

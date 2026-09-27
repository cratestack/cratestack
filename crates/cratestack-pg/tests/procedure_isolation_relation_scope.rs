//! The two advisory fixes compose. A relation filter or relation sort made
//! by a `find_many` inside an `@isolation` procedure runs on the procedure's
//! own transaction (GHSA-r67q-4qqq-g9gm, docs/design/procedure-isolation.md
//! §4) and still splices the related model's read scope into its subquery
//! (GHSA-p55v-6xv5-93p3): a related row the caller cannot read behaves as
//! if it did not exist, exactly as it does outside `@isolation`.
//!
//! Needs a database: `just test-ci-db --test procedure_isolation_relation_scope
//! -- --test-threads=1` with `CRATESTACK_REQUIRE_DB=1`.

mod support;

use cratestack::sqlx::{self, PgPool};
use cratestack::{CratestackContext, CratestackError, Value, include_server_schema};
use support::pg;

include_server_schema!(
    "tests/fixtures/procedure_isolation_relation_scope.cstack",
    db = Postgres
);

use cratestack_schema::procedures as p;
use cratestack_schema::{Cratestack, CreateIrsLinkInput, IsolatedCratestack, Seen, irs_link};

#[derive(Clone)]
struct Procedures;

fn ids(rows: &[cratestack_schema::IrsLink]) -> String {
    rows.iter()
        .map(|row| row.id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

impl p::ProcedureRegistry for Procedures {
    async fn probe(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::probe::Args,
        _authorized: p::probe::Authorized,
    ) -> Result<Seen, CratestackError> {
        // Uncommitted until the attempt commits: only a read on the
        // attempt's own transaction can see it.
        db.irs_link()
            .create(CreateIrsLinkInput {
                id: args.args.id,
                hiddenId: 1,
                mineId: 1,
            })
            .run(ctx)
            .await?;
        let by_hidden = db
            .irs_link()
            .find_many()
            .where_expr(irs_link::hidden().code().eq("A".to_owned()))
            .order_by(irs_link::id().asc())
            .run(ctx)
            .await?;
        let by_mine = db
            .irs_link()
            .find_many()
            .where_expr(irs_link::mine().code().eq("MINE".to_owned()))
            .order_by(irs_link::id().asc())
            .run(ctx)
            .await?;
        let sorted = db
            .irs_link()
            .find_many()
            .order_by(irs_link::hidden().code().desc())
            .order_by(irs_link::id().asc())
            .run(ctx)
            .await?;
        let level = db
            .transaction(async |tx| {
                sqlx::query_scalar::<_, String>("SELECT current_setting('transaction_isolation')")
                    .fetch_one(&mut ***tx)
                    .await
                    .map_err(cratestack::cratestack_error_from_sqlx)
            })
            .await?;
        Ok(Seen {
            byHidden: ids(&by_hidden),
            byMine: ids(&by_mine),
            sortedByHidden: ids(&sorted),
            level,
        })
    }
}

fn caller() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

async fn sql(pool: &PgPool, statement: &str) {
    sqlx::query(sqlx::AssertSqlSafe(statement.to_owned()))
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}

async fn reset(pool: &PgPool) {
    for statement in [
        "DROP TABLE IF EXISTS irs_hiddens, irs_mines, irs_links",
        "CREATE TABLE irs_hiddens (id BIGINT PRIMARY KEY, code TEXT NOT NULL)",
        "CREATE TABLE irs_mines (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, \
         code TEXT NOT NULL)",
        "CREATE TABLE irs_links (id BIGINT PRIMARY KEY, hidden_id BIGINT NOT NULL, \
         mine_id BIGINT NOT NULL)",
        "INSERT INTO irs_hiddens VALUES (1, 'A'), (2, 'B')",
        "INSERT INTO irs_mines VALUES (1, 1, 'MINE')",
        "INSERT INTO irs_links VALUES (1, 1, 1), (2, 2, 1)",
    ] {
        sql(pool, statement).await;
    }
}

/// Unscoped, the hidden filter would match links 1 and 3 (hidden row 1's
/// code is `A`) and the sort would put link 2 (`B`) first. Scoped, hidden
/// row 1 does not exist for the caller: nothing matches, and every sort key
/// reads as `NULL`, leaving the `id` tie-break. The readable relation
/// matches all three links, including the one the attempt just wrote,
/// which shows the reads ran on the attempt's transaction.
#[tokio::test]
async fn relation_filters_inside_an_isolated_procedure_apply_the_related_scope() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    let db = Cratestack::builder(pool.clone()).build();

    let args = p::probe::Args {
        args: cratestack_schema::Step { id: 3 },
    };
    let (call_args, ctx) = (args.clone(), caller());
    let seen = p::probe::invoke_with_db(&db, &args, &caller(), move |tx_db, authorized| {
        let (call_args, ctx) = (call_args.clone(), ctx.clone());
        async move {
            p::ProcedureRegistry::probe(&Procedures, &tx_db, &ctx, call_args, authorized).await
        }
    })
    .await
    .expect("probe");

    assert_eq!(seen.level, "serializable", "ran inside the attempt");
    assert_eq!(seen.byMine, "1,2,3", "readable relation, own write seen");
    assert_eq!(seen.byHidden, "", "matched through a hidden related row");
    assert_eq!(
        seen.sortedByHidden, "1,2,3",
        "sorted by a hidden related row's value"
    );
}

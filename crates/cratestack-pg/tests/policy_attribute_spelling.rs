//! GHSA-69g4-xvcm-vm2j: policy attributes followed by a `//` comment, or
//! sharing a line with another attribute, used to pass `cratestack check`
//! while the generator dropped them — a model with no deny rule, a
//! procedure whose `@authorize` never touched the database, a query that
//! returned rows to the caller its `@deny` names. Each is now applied.
//!
//! `queries_refuse_the_caller_their_deny_names` needs Postgres and skips
//! (prints `ok` in ~0.00s) without one — set `CRATESTACK_REQUIRE_DB=1` to
//! make that a hard failure. The other tests need no database.

use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CratestackContext, CratestackError, Value};

include_server_schema!(
    "tests/fixtures/policy_attribute_spelling.cstack",
    db = Postgres
);

mod support;

use support::pg;

fn caller(role: &str) -> CratestackContext {
    CratestackContext::authenticated([
        ("id".to_owned(), Value::Int(7)),
        ("role".to_owned(), Value::String(role.to_owned())),
    ])
}

#[test]
fn model_deny_rules_followed_by_a_comment_are_generated() {
    let account = &cratestack_schema::models::ACCOUNT_MODEL;
    assert_eq!(account.read_deny_policies.len(), 1);
    assert_eq!(account.detail_deny_policies.len(), 1);
    assert_eq!(account.update_deny_policies.len(), 1);
    assert_eq!(account.delete_deny_policies.len(), 0);
}

#[test]
fn query_deny_rules_followed_by_a_comment_are_generated() {
    use cratestack_schema::queries::{totals_canonical, totals_commented};
    assert_eq!(totals_canonical::DENY_POLICIES.len(), 1);
    assert_eq!(totals_commented::DENY_POLICIES.len(), 1);
}

/// Port 1 on loopback: nothing listens, so a database round trip fails
/// fast. A live `@authorize` must attempt one; a dropped one returned
/// `Ok` without trying.
const DEAD_URL: &str = "postgres://x:x@127.0.0.1:1/x?connect_timeout=2";

#[tokio::test]
async fn authorize_rules_on_a_shared_or_commented_line_consult_the_database() {
    use cratestack_schema::procedures::{transfer, transfer_shared};
    let pool = PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect_lazy(DEAD_URL)
        .expect("a lazy pool never connects up front");
    let db = cratestack_schema::Cratestack::builder(pool).build();
    let input = cratestack_schema::TransferInput {
        accountId: 1,
        amount: 5,
    };
    let args = transfer::Args {
        args: input.clone(),
    };
    let outcome = transfer::authorize_with_db(&db, &args, &caller("user")).await;
    assert!(outcome.is_err(), "`transfer` skipped its @authorize check");
    let args = transfer_shared::Args { args: input };
    let outcome = transfer_shared::authorize_with_db(&db, &args, &caller("user")).await;
    assert!(
        outcome.is_err(),
        "`transferShared` skipped its @authorize check"
    );
}

#[tokio::test]
async fn queries_refuse_the_caller_their_deny_names() {
    use cratestack_schema::queries::{totals_canonical, totals_commented};
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();

    let args = totals_canonical::Args { userId: 42 };
    let banned = db
        .queries()
        .totals_canonical(&args, &caller("banned"))
        .await;
    assert!(
        matches!(banned, Err(CratestackError::Forbidden(_))),
        "{banned:?}"
    );
    let user = db.queries().totals_canonical(&args, &caller("user")).await;
    assert_eq!(user.expect("user is admitted").total, 42);

    let args = totals_commented::Args { userId: 42 };
    let banned = db
        .queries()
        .totals_commented(&args, &caller("banned"))
        .await;
    assert!(
        matches!(banned, Err(CratestackError::Forbidden(_))),
        "{banned:?}"
    );
    let user = db.queries().totals_commented(&args, &caller("user")).await;
    assert_eq!(user.expect("user is admitted").total, 42);
}

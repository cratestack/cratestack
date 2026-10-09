//! The `BigInt` policy fragments against a real Postgres, through the same
//! functions every operation calls: `push_scoped_conditions` (read, and the
//! PK-scoped lookups behind update and delete), `authorize_record_action`
//! (the update and delete preflight) and `push_action_policy_query` spliced
//! into `UPDATE` and `DELETE`. One test, one container: the pool is tied to
//! the runtime that made it, and a container per scenario is minutes.

mod fixture;
mod mutate;
mod read;
mod refuse;
mod undecided;

use super::pg_support::connect_or_skip;

#[tokio::test]
async fn bigint_policies_against_postgres() {
    let Some(pg) = connect_or_skip().await else {
        eprintln!("skipped: no database (set CRATESTACK_USE_TESTCONTAINERS=1 to run)");
        return;
    };
    read::round_trips_and_binds_a_bigint_primary_key(&pg.pool).await;
    read::negated_policies_deny_on_read(&pg.pool).await;
    mutate::negated_policies_deny_on_update_and_delete(&pg.pool).await;
    refuse::a_string_claim_is_refused_not_matched(&pg.pool).await;
    undecided::an_undecidable_claim_denies_on_read_update_and_delete(&pg.pool).await;
    undecided::a_relation_quantifier_never_grants_on_an_undecidable_claim(&pg.pool).await;
}

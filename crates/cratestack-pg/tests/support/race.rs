//! A deterministic "lost conflict race" harness (cratestack#745).
//!
//! Reproducing the upsert race needs one thing that is normally hard to
//! arrange: a conflicting row that is **invisible to the loser's
//! pre-lock probe but visible to its `INSERT`**. Sleeping and hoping the
//! window opens is exactly the flaky-test shape this repo has already
//! paid for (cratestack#417), so this harness gets there with real
//! Postgres blocking instead:
//!
//! 1. A second session opens a transaction and INSERTs the conflicting
//!    row, then **stops** without committing. Under READ COMMITTED that
//!    row is invisible to every other snapshot, and `SELECT ... FOR
//!    UPDATE` cannot even see it to block on it — so the loser's probe
//!    reports "no row" without waiting.
//! 2. The loser's real `INSERT ... ON CONFLICT` *must* block: speculative
//!    insertion waits on the winner's transaction id before it can decide
//!    whether there is a conflict. That block is not a timing window, it
//!    is a hard dependency.
//! 3. Only once the block is **observed** in `pg_stat_activity` does the
//!    harness commit the winner. Observing it is what proves the probe
//!    already ran; if the block never appears the harness panics rather
//!    than letting the test pass having proved nothing.
//!
//! The `sleep` in the poll loop is a polling interval, not a race
//! window: the loop exits on an observed condition or fails loudly.

use std::time::Duration;

use cratestack::sqlx::{Connection, Executor, PgConnection, Row, query};

/// A second session holding an uncommitted conflicting row.
pub struct ConflictWinner {
    winner: PgConnection,
    observer: PgConnection,
    table: &'static str,
}

/// Open an independent session, start a transaction, run `insert_sql`,
/// and leave the transaction open. `table` is the table the *loser* will
/// be seen blocking on.
pub async fn hold_uncommitted_conflict(
    url: &str,
    table: &'static str,
    insert_sql: &str,
) -> ConflictWinner {
    let mut winner = PgConnection::connect(url)
        .await
        .expect("winner session connects");
    let observer = PgConnection::connect(url)
        .await
        .expect("observer session connects");
    winner.execute("BEGIN").await.expect("winner BEGIN");
    winner.execute(insert_sql).await.expect("winner INSERT");
    ConflictWinner {
        winner,
        observer,
        table,
    }
}

impl ConflictWinner {
    /// Block until some other backend is waiting on a lock inside an
    /// `INSERT INTO <table>`, then commit — handing the conflict to a
    /// loser that has provably already finished its probe.
    pub async fn commit_once_loser_blocks(mut self) {
        self.await_blocked_insert().await;
        self.winner.execute("COMMIT").await.expect("winner COMMIT");
    }

    async fn await_blocked_insert(&mut self) {
        let pattern = format!("INSERT INTO {}%", self.table);
        for _ in 0..600 {
            let blocked: i64 = query(
                "SELECT count(*) FROM pg_stat_activity \
                 WHERE pid <> pg_backend_pid() \
                   AND state = 'active' \
                   AND wait_event_type = 'Lock' \
                   AND query ILIKE $1",
            )
            .bind(&pattern)
            .fetch_one(&mut self.observer)
            .await
            .expect("poll pg_stat_activity")
            .get(0);
            if blocked > 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "no backend ever blocked on a lock inside `INSERT INTO {}`: the loser's \
             conflicting INSERT never waited on the winner's transaction, so the \
             ordering this test depends on did not happen and nothing was proved",
            self.table
        );
    }
}

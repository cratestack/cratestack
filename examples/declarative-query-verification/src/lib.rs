//! cratestack#867 verification crate — see `README.md` and this crate's
//! `Cargo.toml` doc comment for why this exists and why it is deliberately
//! **not** a workspace member.
//!
//! This module is the acceptance-bar proof itself: [`monthly_loyalty_fees`]
//! runs the two-aggregate `FILTER (WHERE …)` query epic #488 was opened
//! against, through the generated `Cratestack` handle. Nothing in this file
//! (or anywhere else in this crate's `src/`) imports `sqlx`, names a
//! `sqlx::` path, or writes a line of SQL — the SQL lives in
//! `schema.cstack`, where it is parse-time checked (`$1`/`$2` validated
//! against the declared parameters) and policy-gated.
//!
//! The two things worth noticing about the function below, because they
//! are the difference between this and reaching for `db.pool()`:
//!
//! 1. The `@allow` runs before any SQL does, inside the generated `run` —
//!    there is no unchecked variant to reach for, and a policy-less query
//!    denies everyone.
//! 2. The result is a declared `.cstack` type with real Rust field types,
//!    not a `PgRow` the caller has to `try_get` out of by hand.

use cratestack::{CratestackContext, CratestackError};

cratestack::include_server_schema!("schema.cstack", db = Postgres);

pub use cratestack_schema as schema;

/// Total loyalty-fee discount for one user, plus the slice of it on or
/// after `cutoff`, in a single round trip.
///
/// Two aggregates in one row with a `FILTER` clause on one of them: the
/// generated aggregate builder handles exactly one column and one
/// aggregate per call, so before cratestack#867 this shape had no
/// expression in `.cstack` at all and forced a direct `sqlx` dependency —
/// which is the coupling epic #488 exists to remove.
pub async fn monthly_loyalty_fees(
    db: &schema::Cratestack,
    ctx: &CratestackContext,
    user_id: String,
    cutoff: cratestack::chrono::DateTime<cratestack::chrono::Utc>,
) -> Result<schema::LoyaltyFeeSummary, CratestackError> {
    db.queries()
        .loyalty_fee_summary(
            &schema::queries::loyalty_fee_summary::Args {
                userId: user_id,
                cutoff,
            },
            ctx,
        )
        .await
}

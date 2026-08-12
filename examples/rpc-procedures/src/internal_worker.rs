//! Worked example of the correct way to call a procedure from non-HTTP
//! code — a cron job, background worker, or admin tool — per
//! cratestack#512.
//!
//! `registry.increment(&db, &ctx, args)` (three arguments) is the
//! shape that used to look obvious and skip `@allow` entirely; it no
//! longer compiles at all — see
//! `crates/cratestack-macros/tests/ui_procedure_registry_witness.rs` for
//! the compile-fail proof against this exact schema shape. The function
//! below is the real, sanctioned replacement: it goes through
//! `invoke_with_db`, the same entry point the generated RPC/REST/gRPC
//! dispatch handlers use, so a background job gets identical policy
//! enforcement to an HTTP request — no separate, weaker "internal" path
//! exists to reach for by accident.

use cratestack::{CoolContext, CoolError, SystemContext};

use crate::schema::procedures::ProcedureRegistry;
use crate::{Procedures, schema};

/// Runs `increment` the way a nightly reconciliation job would: attributed
/// to the system principal (`auth().isSystem()`, cratestack#486) rather
/// than any end user. `increment` declares `@allow(auth() != null)`, not a
/// system-specific clause — `SystemContext` is always authenticated
/// (`CoolContext::is_authenticated() == true`), so it satisfies that
/// predicate exactly the way any other authenticated caller's context
/// would, without the job needing to fake a user identity.
pub async fn run_nightly_increment_job(
    procedures: &Procedures,
    db: &schema::Cratestack,
    by: i64,
) -> Result<schema::CounterValue, CoolError> {
    let ctx: CoolContext = SystemContext::for_service("nightly-increment-job").into_context();
    let args = schema::procedures::increment::Args {
        args: schema::CounterDelta { by },
    };

    // `invoke_with_db` runs `@allow`/`@deny` (via `authorize_with_db`)
    // and only then hands the closure the `Authorized` witness that
    // `Procedures::increment`'s last parameter requires — the same
    // sequence `build_router`'s generated dispatch code runs for an HTTP
    // request, just without an HTTP request anywhere in sight.
    let call_args = args.clone();
    let call_ctx = ctx.clone();
    schema::procedures::increment::invoke_with_db(db, &args, &ctx, |authorized| async move {
        procedures
            .increment(db, &call_ctx, call_args, authorized)
            .await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn nightly_job_succeeds_as_the_system_principal() {
        let db = schema::Cratestack::builder().build();
        let procedures = Procedures::default();

        let total = run_nightly_increment_job(&procedures, &db, 3)
            .await
            .expect("system-principal job should pass @allow(auth() != null)");
        assert_eq!(total.total, 3);

        // A second run against the same shared counter proves this is a
        // real call into `Procedures::increment`, not a stub — the state
        // accumulates exactly like an HTTP-triggered `increment` would.
        let total = run_nightly_increment_job(&procedures, &db, 4)
            .await
            .expect("system-principal job should pass @allow(auth() != null)");
        assert_eq!(total.total, 7);
    }
}

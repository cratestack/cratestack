//! GHSA-r67q-4qqq-g9gm: `@isolation("...")` on a procedure must change the
//! transaction its authorization and body run in, on REST and on RPC
//! (MCP: `procedure_isolation_mcp.rs`). docs/design/procedure-isolation.md.
//!
//! Before the fix every procedure below reported `read committed`, and two
//! concurrent declared-serializable withdrawals of 100 from a balance of 100
//! both succeeded.
//!
//! Needs a database. Run it the way CI does — `just test-ci-db --test
//! procedure_isolation -- --test-threads=1` with `CRATESTACK_REQUIRE_DB=1` —
//! or a missing database is a silent skip that still prints `ok` (CLAUDE.md,
//! "Critical test gotcha"). A real run takes seconds; a skip reports 0.00s.

#![cfg(feature = "codec-json")]

mod support;

#[path = "procedure_isolation_support/mod.rs"]
mod iso;

use cratestack::axum::http::StatusCode;
use iso::{Gate, account_balances, audit_rows, post, reset};
use support::pg;

const TRANSPORTS: [(&str, bool); 2] = [("REST", false), ("RPC", true)];

fn uri(rpc: bool, procedure: &str) -> String {
    if rpc {
        format!("/rpc/procedure.{procedure}")
    } else {
        format!("/$procs/{procedure}")
    }
}

#[tokio::test]
async fn declared_isolation_is_the_level_the_procedure_runs_at() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[]).await;
    // Not vacuous: the server's own default is READ COMMITTED, so anything
    // else below came from the procedure's transaction.
    let default: String = cratestack::sqlx::query_scalar("SHOW default_transaction_isolation")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(default, "read committed");

    let body = r#"{"args":{"nonce":"x"}}"#;
    for (label, rpc) in TRANSPORTS {
        for (procedure, expected) in [
            ("levelSerializable", "serializable"),
            ("levelRepeatableRead", "repeatable read"),
            ("levelReadCommitted", "read committed"),
            ("levelPlain", "read committed"),
        ] {
            let (router, _) = iso::routers(&pool, Gate::new(1), None, rpc);
            let (status, text) = post(router, uri(rpc, procedure), body).await;
            println!("ISOLATION {label} {procedure}: {status} {text}");
            assert_eq!(status, StatusCode::OK, "{label} {procedure}: {text}");
            assert!(
                text.contains(&format!(r#""level":"{expected}""#)),
                "{label} {procedure} must run at {expected}: {text}"
            );
        }
    }
}

/// `/rpc/batch` dispatches each frame through the same per-procedure
/// handler, so each frame gets its own transaction at its own level.
#[tokio::test]
async fn rpc_batch_frames_run_at_their_declared_levels() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[]).await;
    let (router, _) = iso::routers(&pool, Gate::new(1), None, true);
    let body = r#"[
        {"id":1,"op":"procedure.levelSerializable","input":{"args":{"nonce":"a"}}},
        {"id":2,"op":"procedure.levelRepeatableRead","input":{"args":{"nonce":"b"}}},
        {"id":3,"op":"procedure.levelPlain","input":{"args":{"nonce":"c"}}}
    ]"#;
    let (status, text) = post(router, "/rpc/batch".to_owned(), body).await;
    println!("ISOLATION RPC batch: {status} {text}");
    assert_eq!(status, StatusCode::OK, "{text}");
    let frames: serde_json::Value = serde_json::from_str(&text).unwrap();
    let level = |id: u64| {
        frames
            .as_array()
            .unwrap()
            .iter()
            .find(|frame| frame["id"] == id)
            .map(|frame| frame["output"]["level"].clone())
    };
    assert_eq!(level(1), Some("serializable".into()), "{text}");
    assert_eq!(level(2), Some("repeatable read".into()), "{text}");
    assert_eq!(level(3), Some("read committed".into()), "{text}");
}

#[tokio::test]
async fn concurrent_withdrawals_never_overdraw_one_account() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    let body = r#"{"args":{"accountId":1,"amount":100}}"#;

    for (label, rpc) in TRANSPORTS {
        // Control: no `@isolation`. Both callers read 100 before either
        // writes (the gate), both pass the check, both debit: 200 paid out
        // of 100. If this stopped happening, the test below would prove
        // nothing.
        reset(&pool, &[(1, 100)]).await;
        let (router, _) = iso::routers(&pool, Gate::new(2), None, rpc);
        let (a, b) = tokio::join!(
            post(router.clone(), uri(rpc, "withdrawPlain"), body),
            post(router.clone(), uri(rpc, "withdrawPlain"), body),
        );
        println!("CONTROL {label} withdrawPlain A: {} {}", a.0, a.1);
        println!("CONTROL {label} withdrawPlain B: {} {}", b.0, b.1);
        assert_eq!((a.0, b.0), (StatusCode::OK, StatusCode::OK), "control");

        // Declared serializable: one debit lands; the other attempt fails
        // with 40001, is retried, reads 0 and is refused by the check.
        reset(&pool, &[(1, 100)]).await;
        let sink = iso::RecordingSink::default();
        let (router, registry) = iso::routers(&pool, Gate::new(2), Some(sink.clone()), rpc);
        let (a, b) = tokio::join!(
            post(router.clone(), uri(rpc, "withdraw"), body),
            post(router.clone(), uri(rpc, "withdraw"), body),
        );
        let balances = account_balances(&pool).await;
        let runs = registry.runs();
        println!("ISOLATED {label} withdraw A: {} {}", a.0, a.1);
        println!("ISOLATED {label} withdraw B: {} {}", b.0, b.1);
        println!("ISOLATED {label} balances={balances:?} body runs={runs}");
        let oks = [&a, &b].iter().filter(|r| r.0 == StatusCode::OK).count();
        assert_eq!(oks, 1, "exactly one withdrawal succeeds: {a:?} {b:?}");
        let refused = if a.0 == StatusCode::OK { &b } else { &a };
        assert!(
            refused.1.contains("insufficient funds"),
            "the loser is refused by the balance check after its retry: {refused:?}"
        );
        assert_eq!(balances, vec![(1, 0)], "exactly one debit of 100");
        assert!(runs >= 3, "the losing body ran again after 40001: {runs}");
        assert_eq!(audit_rows(&pool).await, 1, "one committed, audited debit");
        assert_eq!(sink.len(), 1, "AuditSink saw only the committed attempt");
        assert_eq!(registry.delivered(), 1, "one @@emit event, after commit");
    }
}

/// The loser's debit fails with 40001 and the body swallows it. The attempt
/// saw a serialization failure, so it is rolled back and retried rather than
/// committed as a success that debited nothing.
#[tokio::test]
async fn a_swallowed_serialization_failure_is_still_retried() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    let body = r#"{"args":{"accountId":1,"amount":100}}"#;

    for (label, rpc) in TRANSPORTS {
        reset(&pool, &[(1, 100)]).await;
        let (router, registry) = iso::routers(&pool, Gate::new(2), None, rpc);
        let (a, b) = tokio::join!(
            post(router.clone(), uri(rpc, "withdrawSwallow"), body),
            post(router.clone(), uri(rpc, "withdrawSwallow"), body),
        );
        println!("SWALLOW {label} A: {} {}", a.0, a.1);
        println!("SWALLOW {label} B: {} {}", b.0, b.1);
        let oks = [&a, &b].iter().filter(|r| r.0 == StatusCode::OK).count();
        assert_eq!(
            oks, 1,
            "no success reported for a debit that failed: {a:?} {b:?}"
        );
        assert!(registry.runs() >= 3, "the swallowing attempt was retried");
        assert_eq!(account_balances(&pool).await, vec![(1, 0)]);
    }
}

#[tokio::test]
async fn write_skew_across_two_accounts_is_refused() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();

    for (label, rpc) in TRANSPORTS {
        // Rule: the SUM over both accounts may not go negative. Each caller
        // checks the sum and debits a *different* row, so no row lock is
        // ever contended; only SERIALIZABLE sees the dependency.
        reset(&pool, &[(1, 50), (2, 50)]).await;
        let sink = iso::RecordingSink::default();
        let (router, registry) = iso::routers(&pool, Gate::new(2), Some(sink.clone()), rpc);
        let (a, b) = tokio::join!(
            post(
                router.clone(),
                uri(rpc, "withdrawJoint"),
                r#"{"args":{"accountId":1,"amount":100}}"#
            ),
            post(
                router.clone(),
                uri(rpc, "withdrawJoint"),
                r#"{"args":{"accountId":2,"amount":100}}"#
            ),
        );
        let balances = account_balances(&pool).await;
        let total: i64 = balances.iter().map(|(_, balance)| balance).sum();
        println!("SKEW {label} A: {} {}", a.0, a.1);
        println!("SKEW {label} B: {} {}", b.0, b.1);
        println!(
            "SKEW {label} balances={balances:?} total={total} runs={} sink={}",
            registry.runs(),
            sink.len()
        );
        let oks = [&a, &b].iter().filter(|r| r.0 == StatusCode::OK).count();
        assert_eq!(oks, 1, "exactly one joint withdrawal succeeds: {a:?} {b:?}");
        assert_eq!(total, 0, "the sum never goes negative");
        assert!(registry.runs() >= 3, "the loser was retried");
        assert_eq!(audit_rows(&pool).await, 1);
        assert_eq!(
            sink.len(),
            1,
            "no AuditSink event for a rolled-back attempt"
        );
        assert_eq!(
            registry.delivered(),
            1,
            "no @@emit event for a rolled-back attempt"
        );
    }
}

#[tokio::test]
async fn nothing_a_failed_isolated_procedure_wrote_survives() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();

    for (label, rpc) in TRANSPORTS {
        reset(&pool, &[]).await;
        let sink = iso::RecordingSink::default();
        let (router, registry) = iso::routers(&pool, Gate::new(1), Some(sink.clone()), rpc);
        let (status, text) = post(
            router,
            uri(rpc, "writeThenFail"),
            r#"{"args":{"accountId":70,"amount":5}}"#,
        )
        .await;
        let seen = registry.seen();
        println!("ESCAPE {label}: {status} {text} seen-inside={seen:?}");
        assert!(text.contains("forced failure"), "{text}");
        // Inside the body, the ORM write (id 70), the raw write (id 71) and
        // the `run_in_tx` write (id 72) were all visible to the ORM read and
        // to raw SQL: one transaction.
        assert_eq!(seen.model_saw_70, Some(true), "{seen:?}");
        assert_eq!(seen.raw_count, Some(3), "{seen:?}");
        // The handle refuses re-entrant use instead of reaching around the
        // transaction it is inside of.
        assert_eq!(
            seen.reentrant_code.as_deref(),
            Some("INTERNAL_ERROR"),
            "{seen:?}"
        );
        // After: none of it committed — not through the ORM, not through
        // raw SQL, not the audit row, not the AuditSink.
        assert_eq!(
            account_balances(&pool).await,
            vec![],
            "{label}: rows survived"
        );
        assert_eq!(audit_rows(&pool).await, 0);
        assert_eq!(sink.len(), 0);
    }
}

/// ADR 0019 D5 (PR A): a validator on an argument `type` refuses the call
/// from inside `authorize_with_db`, which an `@isolation` procedure reaches
/// in its attempt's transaction and a plain one reaches on the pool. Either
/// way the answer is the 422 a failed model validator gives (`VALIDATION_ERROR`
/// on REST, `invalid_argument` on RPC), never the `TRANSACTION_ABORTED` a
/// retried attempt ends in, and the body never runs. `withdraw` also carries
/// an `@authorize` model check, which validation precedes.
#[tokio::test]
async fn a_type_validator_refuses_an_isolated_procedures_arguments_before_it_runs() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();

    for (label, rpc) in TRANSPORTS {
        for procedure in ["withdraw", "withdrawPlain"] {
            reset(&pool, &[(1, 100)]).await;
            let (router, registry) = iso::routers(&pool, Gate::new(1), None, rpc);
            let (status, text) = post(
                router,
                uri(rpc, procedure),
                r#"{"args":{"accountId":1,"amount":0}}"#,
            )
            .await;
            println!("VALIDATION {label} {procedure}: {status} {text}");
            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{label} {procedure}: {text}"
            );
            let code = if rpc {
                "invalid_argument"
            } else {
                "VALIDATION_ERROR"
            };
            assert!(text.contains(&format!(r#""code":"{code}""#)), "{text}");
            assert!(
                text.contains("field 'args.amount' is below minimum 1"),
                "{text}"
            );
            assert_eq!(registry.runs(), 0, "{label} {procedure}: the body ran");
            assert_eq!(account_balances(&pool).await, vec![(1, 100)]);

            // The control: the same call with a valid amount runs and pays.
            reset(&pool, &[(1, 100)]).await;
            let (router, registry) = iso::routers(&pool, Gate::new(1), None, rpc);
            let (status, text) = post(
                router,
                uri(rpc, procedure),
                r#"{"args":{"accountId":1,"amount":100}}"#,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{label} {procedure}: {text}");
            assert_eq!(registry.runs(), 1);
            assert_eq!(account_balances(&pool).await, vec![(1, 0)]);
        }
    }
}

#[tokio::test]
async fn exhausted_retries_are_aborted_not_an_overdraft() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    let body = r#"{"args":{"accountId":1,"amount":100}}"#;

    for (label, rpc) in TRANSPORTS {
        reset(&pool, &[(1, 100)]).await;
        let (router, registry) = iso::routers_with_retries(&pool, Gate::new(2), 0, rpc);
        let (a, b) = tokio::join!(
            post(router.clone(), uri(rpc, "withdraw"), body),
            post(router.clone(), uri(rpc, "withdraw"), body),
        );
        println!("NO-RETRY {label} A: {} {}", a.0, a.1);
        println!("NO-RETRY {label} B: {} {}", b.0, b.1);
        let oks = [&a, &b].iter().filter(|r| r.0 == StatusCode::OK).count();
        assert_eq!(oks, 1, "{a:?} {b:?}");
        let refused = if a.0 == StatusCode::OK { &b } else { &a };
        // 409 on both, with its own code — `TRANSACTION_ABORTED` on REST,
        // the RPC binding's `aborted` — never the `CONFLICT` a unique
        // violation carries.
        assert_eq!(refused.0, StatusCode::CONFLICT, "{refused:?}");
        let code = if rpc {
            r#""code":"aborted""#
        } else {
            r#""code":"TRANSACTION_ABORTED""#
        };
        assert!(refused.1.contains(code), "{refused:?}");
        assert!(
            refused.1.contains("concurrent updates; retry the request"),
            "fixed public detail, no driver text: {refused:?}"
        );
        assert_eq!(registry.runs(), 2, "retry budget 0: no second attempt");
        assert_eq!(account_balances(&pool).await, vec![(1, 0)]);
    }
}

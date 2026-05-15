//! Deterministic in-process tests for `BatchDebouncer`. No timers, no
//! ports — the tests rely on size-based auto-flush + explicit
//! `flush()` calls.

use rpc_batch_debounce_example::{build_router, BatchDebouncer};

#[tokio::test]
async fn fewer_than_max_size_calls_wait_for_explicit_flush() {
    // max_size = 4. Issue 3 calls — none should flush automatically.
    let service = build_router();
    let debouncer = BatchDebouncer::new(service, 4).with_auth_id(1);

    let d = debouncer.clone();
    let h1 = tokio::spawn(async move {
        d.call("procedure.add", serde_json::json!({"args": {"a": 1, "b": 2}}))
            .await
    });
    let d = debouncer.clone();
    let h2 = tokio::spawn(async move {
        d.call(
            "procedure.multiply",
            serde_json::json!({"args": {"a": 3, "b": 4}}),
        )
        .await
    });
    let d = debouncer.clone();
    let h3 = tokio::spawn(async move {
        d.call(
            "procedure.add",
            serde_json::json!({"args": {"a": 10, "b": 20}}),
        )
        .await
    });

    // Give the spawned tasks a chance to register their pending frames
    // — yield round-trip is enough on the multi-thread runtime.
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert_eq!(debouncer.pending_len().await, 3, "all 3 calls queued");

    // No one has flushed; the three tasks are still parked on their
    // oneshot receivers. Explicit flush drives them.
    debouncer.flush().await.expect("flush succeeds");

    let r1 = h1.await.unwrap().unwrap();
    let r2 = h2.await.unwrap().unwrap();
    let r3 = h3.await.unwrap().unwrap();

    assert_eq!(r1.output.unwrap()["value"], 3);
    assert_eq!(r2.output.unwrap()["value"], 12);
    assert_eq!(r3.output.unwrap()["value"], 30);

    // Buffer drained.
    assert_eq!(debouncer.pending_len().await, 0);
}

#[tokio::test]
async fn hitting_max_size_triggers_auto_flush() {
    // max_size = 2. Two calls in a row trigger a flush after the second
    // one is enqueued — both awaits resolve without anyone calling flush().
    let service = build_router();
    let debouncer = BatchDebouncer::new(service, 2).with_auth_id(1);

    let d = debouncer.clone();
    let h1 = tokio::spawn(async move {
        d.call(
            "procedure.add",
            serde_json::json!({"args": {"a": 100, "b": 100}}),
        )
        .await
    });

    let d = debouncer.clone();
    let h2 = tokio::spawn(async move {
        d.call(
            "procedure.add",
            serde_json::json!({"args": {"a": 200, "b": 200}}),
        )
        .await
    });

    // Both should resolve without anyone calling flush() — the second
    // enqueue trips the size limit.
    let r1 = h1.await.unwrap().unwrap();
    let r2 = h2.await.unwrap().unwrap();
    assert_eq!(r1.output.unwrap()["value"], 200);
    assert_eq!(r2.output.unwrap()["value"], 400);
}

#[tokio::test]
async fn per_call_errors_route_to_the_right_awaiter() {
    // Mix of three calls — second one will fail (divide by zero). Each
    // caller awaits its own result; the error must land on the second
    // task's oneshot, not contaminate the others.
    let service = build_router();
    let debouncer = BatchDebouncer::new(service, 3).with_auth_id(1);

    let d = debouncer.clone();
    let h1 = tokio::spawn(async move {
        d.call(
            "procedure.add",
            serde_json::json!({"args": {"a": 1, "b": 1}}),
        )
        .await
    });
    let d = debouncer.clone();
    let h2 = tokio::spawn(async move {
        d.call(
            "procedure.divide",
            serde_json::json!({"args": {"numerator": 10, "denominator": 0}}),
        )
        .await
    });
    let d = debouncer.clone();
    let h3 = tokio::spawn(async move {
        d.call(
            "procedure.multiply",
            serde_json::json!({"args": {"a": 6, "b": 7}}),
        )
        .await
    });

    // Three calls + max_size=3 → auto-flush on the third enqueue.
    let r1 = h1.await.unwrap().unwrap();
    let r2 = h2.await.unwrap().unwrap();
    let r3 = h3.await.unwrap().unwrap();

    assert!(r1.error.is_none());
    assert_eq!(r1.output.unwrap()["value"], 2);

    // Per-frame error didn't contaminate other awaiters.
    let err = r2.error.expect("frame 2 should error");
    assert_eq!(err.code, "failed_precondition");

    assert!(r3.error.is_none());
    assert_eq!(r3.output.unwrap()["value"], 42);
}

#[tokio::test]
async fn empty_flush_is_a_noop() {
    let service = build_router();
    let debouncer = BatchDebouncer::new(service, 4).with_auth_id(1);

    assert_eq!(debouncer.pending_len().await, 0);
    debouncer.flush().await.expect("empty flush succeeds");
    assert_eq!(debouncer.pending_len().await, 0);
}

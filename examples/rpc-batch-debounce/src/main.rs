//! End-to-end demo: spin up an RPC server in-process, point a debouncer
//! at it, fire many independent `.call()` invocations from concurrent
//! tasks, watch them coalesce into one batch round-trip.
//!
//! Run:
//!
//! ```bash
//! cargo run -p rpc-batch-debounce-example
//! ```
//!
//! The expected output: 12 calls go in (across 3 procedures), 1 batch
//! HTTP request goes out, 12 results come back correctly mapped.

use rpc_batch_debounce_example::{build_router, BatchDebouncer};
use std::time::Instant;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // Build the in-process server. In production this would be a real
    // network client pointing at an upstream service.
    let service = build_router();

    // 4-call window. Two batches will land for 12 issued calls.
    let debouncer = BatchDebouncer::new(service, 4).with_auth_id(1);

    println!("Issuing 12 .call() invocations with a 4-call debouncer:");
    let start = Instant::now();

    // Fan out 12 concurrent calls — these would be from different parts
    // of the UI / different worker tasks in a real app.
    let mut handles = Vec::new();
    for i in 0..12 {
        let d = debouncer.clone();
        handles.push(tokio::spawn(async move {
            let (op, input) = match i % 3 {
                0 => (
                    "procedure.add",
                    serde_json::json!({"args": {"a": i, "b": i}}),
                ),
                1 => (
                    "procedure.multiply",
                    serde_json::json!({"args": {"a": i, "b": 2}}),
                ),
                _ => (
                    "procedure.divide",
                    serde_json::json!({"args": {"numerator": i, "denominator": 1.max(i % 5)}}),
                ),
            };
            let frame = d.call(op, input).await.expect("debouncer.call");
            (i, op, frame)
        }));
    }

    for h in handles {
        let (i, op, frame) = h.await.expect("task");
        match (frame.output, frame.error) {
            (Some(out), None) => println!("  call {i:2} ({op:20}) -> {out}"),
            (None, Some(err)) => println!("  call {i:2} ({op:20}) -> ERROR {err:?}"),
            _ => unreachable!(),
        }
    }

    let elapsed = start.elapsed();
    println!();
    println!("Done in {elapsed:?}. Three batches landed (12 calls / 4-call window).");
    println!("In a real app the debouncer would be wrapping a network client; the");
    println!("savings show up as fewer round-trips on a flaky / metered link.");
}

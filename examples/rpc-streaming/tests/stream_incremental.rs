//! Proves `Procedures::ticks` is genuinely incremental at the
//! `ProcedureRegistry` boundary — cratestack#282's own acceptance
//! criterion ("a test that polls the stream ... and asserts it doesn't
//! block waiting for ALL items before yielding the first one"), distinct
//! from (and prior to) HTTP-wire incrementality, which cratestack#283
//! will add.
//!
//! This talks to `Procedures::ticks` directly, not through
//! `build_router`/HTTP — at the time this test was written (cratestack#282)
//! the HTTP path still buffered the whole sequence before responding, so
//! an HTTP-level timing test could only ever have measured buffering,
//! not streaming; polling the trait method's returned `Stream` directly
//! was the only place incrementality was observable at all. HTTP-level
//! incrementality shipped in cratestack#283 — see
//! `tests/stream_wire_timing.rs` for the equivalent proof against the
//! real generated router over a real HTTP response. Both tests are kept:
//! this one still pins the trait-boundary guarantee independently of the
//! transport layer built on top of it.

use std::time::Instant;

use cratestack::futures::StreamExt;
use rpc_streaming_example::schema::procedures::ProcedureRegistry;
use rpc_streaming_example::{Procedures, build_db, schema};

const COUNT: i64 = 5;

#[tokio::test]
async fn ticks_stream_yields_first_item_well_before_the_stream_completes() {
    let db = build_db();
    // `ticks` declares `@allow(auth() != null)` — an anonymous context
    // would (correctly, post-cratestack#512) be denied by `authorize_with_db`
    // below, so this test authenticates rather than proving the wrong thing.
    let ctx =
        cratestack::CoolContext::authenticated([("id".to_owned(), cratestack::Value::Int(1))]);
    let args = schema::procedures::ticks::Args {
        args: schema::TickerArgs {
            start: 0,
            count: COUNT,
        },
    };

    // cratestack#512: `Procedures::ticks` now takes an `Authorized` witness
    // that only `authorize_with_db`/`invoke_with_db` can construct — obtain
    // one the same way the generated dispatch code does, rather than
    // calling the trait method directly with no policy check at all.
    let authorized = schema::procedures::ticks::authorize_with_db(&db, &args, &ctx)
        .await
        .expect("authenticated caller should pass @allow(auth() != null)");

    // `Procedures::ticks` returns `impl Stream<..> + Send`, not
    // necessarily `Unpin` (the `async_stream::stream!`-generated state
    // machine isn't) — `Box::pin` gets us something `StreamExt::next` can
    // poll without pinning it to the stack by hand.
    let procedures = Procedures::default();
    let mut stream = Box::pin(procedures.ticks(&db, &ctx, args, authorized));

    let started = Instant::now();
    let first = stream
        .next()
        .await
        .expect("stream should yield a first item")
        .expect("first item should decode as Ok");
    let first_elapsed = started.elapsed();
    assert_eq!(first.index, 0);
    assert_eq!(first.value, 0);

    let mut last_index = first.index;
    while let Some(item) = stream.next().await {
        last_index = item.expect("every yielded item should be Ok").index;
    }
    let total_elapsed = started.elapsed();
    assert_eq!(
        last_index,
        COUNT - 1,
        "stream should yield exactly COUNT items, 0-indexed"
    );

    // The whole point of `@stream`: the first item is ready long before
    // the full sequence is. If `ticks` still collected into a `Vec`
    // first (the pre-#282, non-`@stream` shape), `first_elapsed` would be
    // indistinguishable from `total_elapsed` — both would be "wait for
    // all COUNT items". A generous factor keeps this from being flaky
    // under CI scheduling jitter while still failing loudly if
    // incremental production regresses back into buffering.
    assert!(
        first_elapsed < total_elapsed / 2,
        "first item ({first_elapsed:?}) should arrive well before the full \
         stream completes ({total_elapsed:?}) — if this fails, `ticks` is \
         buffering again instead of streaming incrementally",
    );
}

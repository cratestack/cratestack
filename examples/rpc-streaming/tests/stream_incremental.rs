//! Proves `Procedures::ticks` is genuinely incremental at the
//! `ProcedureRegistry` boundary — cratestack#282's own acceptance
//! criterion ("a test that polls the stream ... and asserts it doesn't
//! block waiting for ALL items before yielding the first one"), distinct
//! from (and prior to) HTTP-wire incrementality, which cratestack#283
//! will add.
//!
//! This talks to `Procedures::ticks` directly, not through
//! `build_router`/HTTP: the HTTP path still buffers the whole sequence
//! before responding (see `procedure_invoke_call_tokens` in
//! `cratestack-macros` — that's `@stream`'s deliberate scope boundary for
//! this ticket, not an oversight), so an HTTP-level timing test could
//! only ever measure buffering, not streaming. Polling the trait method's
//! returned `Stream` directly is the only place in the system right now
//! where incrementality is actually observable.

use std::time::Instant;

use cratestack::futures::StreamExt;
use rpc_streaming_example::schema::procedures::ProcedureRegistry;
use rpc_streaming_example::{Procedures, build_db, schema};

const COUNT: i64 = 5;

#[tokio::test]
async fn ticks_stream_yields_first_item_well_before_the_stream_completes() {
    let db = build_db();
    let ctx = cratestack::CoolContext::anonymous();
    let args = schema::procedures::ticks::Args {
        args: schema::TickerArgs {
            start: 0,
            count: COUNT,
        },
    };

    // `Procedures::ticks` returns `impl Stream<..> + Send`, not
    // necessarily `Unpin` (the `async_stream::stream!`-generated state
    // machine isn't) — `Box::pin` gets us something `StreamExt::next` can
    // poll without pinning it to the stack by hand.
    let mut stream = Box::pin(Procedures.ticks(&db, &ctx, args));

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

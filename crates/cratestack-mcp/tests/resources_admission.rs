//! Rate-limit admission on the *record* read path (cratestack#1040), which
//! `tests/resources.rs` covers only for the collection path: its throttle
//! lands on a page read, so a record read that queried the table before it
//! was admitted went unnoticed (mutation: swap `admit` and `read_record` in
//! `src/resources/read.rs`; every existing test passed).
//!
//! Both halves matter for the same reason. Admission is the only thing
//! standing between an agent and the database, so a throttled read must not
//! run its query; and a read that finds nothing must still be charged, as
//! REST charges a 404, or probing ids would be free.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cratestack_core::{RateLimitConfig, RateLimitStore};
use cratestack_mcp::{OpExecutor, StdioServer};
use serde_json::json;
use support::client::Client;
use support::resources::FakeResources;
use support::stores::CountingLimiter;
use support::user;

#[tokio::test]
async fn a_missing_record_is_charged_and_a_throttled_one_never_reaches_the_table() {
    let table = FakeResources::default();
    let limiter = Arc::new(CountingLimiter::new(1));
    let executor = OpExecutor::new(None, Duration::ZERO).with_rate_limit(
        limiter.clone() as Arc<dyn RateLimitStore>,
        RateLimitConfig::new(1, 0.0),
    );
    let server = StdioServer::new(table.clone(), user("u-1"))
        .unwrap()
        .with_executor(executor);
    let mut client = Client::start(server);

    let missing = client
        .request(
            "resources/read",
            json!({ "uri": "cratestack://blog/posts/99999" }),
        )
        .await;
    assert_eq!(
        missing["error"]["message"], "resource not found",
        "{missing}"
    );
    assert_eq!(
        limiter.charged.load(Ordering::SeqCst),
        1,
        "a read that found nothing is still charged"
    );

    let throttled = client
        .request(
            "resources/read",
            json!({ "uri": "cratestack://blog/posts/1" }),
        )
        .await;
    assert_eq!(throttled["error"]["code"], json!(-32603), "{throttled}");
    assert_eq!(throttled["error"]["data"]["code"], "TOO_MANY_REQUESTS");
    assert_eq!(
        table.reads(),
        1,
        "only the admitted read reached the table; the throttled one never ran"
    );
    client.close().await.unwrap();
}

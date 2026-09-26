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

/// Maintainer decision 3 on #1040, kept rather than changed. A collection
/// read whose `limit` or cursor can never be served is refused (`-32602`)
/// *before* admission, so it costs the caller nothing: the reason tool
/// arguments decode before admission. A NUL or unparsable id is instead
/// the same "resource not found" a missing row gets, so the answer says
/// nothing more about the id than a missing row's would.
#[tokio::test]
async fn a_malformed_page_query_is_never_charged_and_a_bad_id_is_not_found() {
    let limiter = Arc::new(CountingLimiter::new(100));
    let executor = OpExecutor::new(None, Duration::ZERO).with_rate_limit(
        limiter.clone() as Arc<dyn RateLimitStore>,
        RateLimitConfig::new(100, 0.0),
    );
    let server = StdioServer::new(FakeResources::default(), user("u-1"))
        .unwrap()
        .with_executor(executor);
    let mut client = Client::start(server);

    for uri in [
        "cratestack://blog/posts?limit=0",
        "cratestack://blog/posts?limit=ten",
        "cratestack://blog/posts?cursor=garbage",
    ] {
        let refused = client
            .request("resources/read", json!({ "uri": uri }))
            .await;
        assert_eq!(refused["error"]["code"], json!(-32602), "{uri}: {refused}");
        assert_ne!(refused["error"]["message"], "resource not found", "{uri}");
    }
    assert_eq!(
        limiter.charged.load(Ordering::SeqCst),
        0,
        "a refused page query is never charged"
    );

    let missing = client
        .request(
            "resources/read",
            json!({ "uri": "cratestack://blog/posts/99999" }),
        )
        .await;
    for uri in [
        "cratestack://blog/posts/abc",
        "cratestack://blog/posts/1%00",
    ] {
        let bad = client
            .request("resources/read", json!({ "uri": uri }))
            .await;
        assert_eq!(bad["error"], missing["error"], "{uri}: {bad}");
    }
    client.close().await.unwrap();
}

/// Which bad ids cost a token, kept as it was built (maintainer decision on
/// cratestack#1033, answering #1068's question). What the URI parser can
/// refuse on its own (an unknown segment, a raw character outside RFC 3986's
/// `pchar`, a NUL) is refused before admission and costs nothing. An id
/// that is a well-formed segment but no key of the model's type (`abc` for
/// an `Int` key) is only found out inside the generated read, after
/// admission, so it is charged like a missing row. Every one answers the
/// same "resource not found", so the difference in cost says something
/// about the id's type, never about whether a row exists.
#[tokio::test]
async fn what_a_bad_id_costs() {
    let table = FakeResources::default();
    let limiter = Arc::new(CountingLimiter::new(100));
    let executor = OpExecutor::new(None, Duration::ZERO).with_rate_limit(
        limiter.clone() as Arc<dyn RateLimitStore>,
        RateLimitConfig::new(100, 0.0),
    );
    let server = StdioServer::new(table.clone(), user("u-1"))
        .unwrap()
        .with_executor(executor);
    let mut client = Client::start(server);
    let mut read = async |uri: &str| {
        client
            .request("resources/read", json!({ "uri": uri }))
            .await
    };

    let missing = read("cratestack://blog/posts/99999").await;
    assert_eq!(limiter.charged.load(Ordering::SeqCst), 1, "{missing}");
    for free in [
        "cratestack://blog/nope/1",
        "cratestack://blog/posts/a b",
        "cratestack://blog/posts/1%00",
    ] {
        assert_eq!(read(free).await["error"], missing["error"], "{free}");
    }
    assert_eq!(
        limiter.charged.load(Ordering::SeqCst),
        1,
        "what the URI parser refuses is never charged"
    );
    assert_eq!(table.reads(), 1, "and never reaches the table");

    let unparsable = read("cratestack://blog/posts/abc").await;
    assert_eq!(unparsable["error"], missing["error"], "{unparsable}");
    assert_eq!(
        limiter.charged.load(Ordering::SeqCst),
        2,
        "an id only the read can reject is charged like a missing row"
    );
    client.close().await.unwrap();
}

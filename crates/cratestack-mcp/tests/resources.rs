//! The resource wire contract (cratestack#1040), driven as raw JSON-RPC
//! over an in-memory duplex against a hand-written table: listing, the
//! identical not-found error, page sizes, cursors, and admission.
//!
//! Row visibility itself is the generated table's job and is proven against
//! Postgres in `cratestack-pg`'s `tests/mcp_resources_pg.rs`; the fake here
//! filters before paging exactly as that SQL does.

mod support;

use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{RateLimitConfig, RateLimitStore};
use cratestack_mcp::{OpExecutor, StdioServer};
use serde_json::{Value, json};
use support::client::Client;
use support::resources::{FakeResources, ROWS, visible};
use support::stores::CountingLimiter;
use support::{FakeTools, user};

fn client(table: FakeResources) -> Client {
    Client::start(StdioServer::new(table, user("u-1")).unwrap())
}

async fn read(client: &mut Client, uri: &str) -> Value {
    client
        .request("resources/read", json!({ "uri": uri }))
        .await
}

/// The JSON document of a successful read.
fn document(response: &Value) -> Value {
    let contents = response["result"]["contents"]
        .as_array()
        .unwrap_or_else(|| panic!("not a read result: {response}"));
    assert_eq!(contents.len(), 1, "{response}");
    assert_eq!(contents[0]["mimeType"], "application/json");
    serde_json::from_str(contents[0]["text"].as_str().unwrap()).unwrap()
}

fn ids(page: &Value) -> Vec<u64> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_u64().unwrap())
        .collect()
}

fn visible_ids() -> Vec<u64> {
    (1..=ROWS).filter(|id| visible(*id)).collect()
}

#[tokio::test]
async fn discovery_and_listings_name_segments_never_models() {
    let mut client = client(FakeResources::default());
    let discover = client.request("server/discover", json!({})).await;
    assert!(
        discover["result"]["capabilities"]["resources"].is_object(),
        "{discover}"
    );

    let list = client.request("resources/list", json!({})).await;
    let uris: Vec<&str> = list["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|resource| resource["uri"].as_str().unwrap())
        .collect();
    assert_eq!(uris, ["cratestack://blog/posts", "cratestack://blog/notes"]);
    assert_eq!(list["result"]["ttlMs"], json!(0));
    assert_eq!(list["result"]["cacheScope"], "private");

    let templates = client.request("resources/templates/list", json!({})).await;
    let templates: Vec<&str> = templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|template| template["uriTemplate"].as_str().unwrap())
        .collect();
    assert_eq!(
        templates,
        [
            "cratestack://blog/posts/{id}",
            "cratestack://blog/posts{?limit,cursor}",
            "cratestack://blog/notes/{id}",
            "cratestack://blog/notes{?limit,cursor}",
        ]
    );
    for listing in [&list, &discover] {
        let text = listing.to_string();
        assert!(!text.contains("Post") && !text.contains("model."), "{text}");
    }
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_record_read_is_one_private_json_block() {
    let mut client = client(FakeResources::default());
    let response = read(&mut client, "cratestack://blog/posts/1").await;
    assert_eq!(document(&response), json!({ "id": 1, "segment": "posts" }));
    let result = &response["result"];
    assert_eq!(result["contents"][0]["uri"], "cratestack://blog/posts/1");
    assert_eq!(result["ttlMs"], json!(0), "ttlMs is set");
    assert_eq!(result["cacheScope"], "private");
    client.close().await.unwrap();
}

/// RFC 3986 § 3.1 (maintainer decision on #1040): `CRATESTACK://` is the
/// same URI as `cratestack://`, and reads the same record. The name after
/// it is not case-folded: `cratestack://BLOG/` is the not-found a missing
/// row gets.
#[tokio::test]
async fn the_scheme_is_case_insensitive_and_the_name_is_not() {
    let mut client = client(FakeResources::default());
    for uri in ["CRATESTACK://blog/posts/1", "Cratestack://blog/posts/1"] {
        let response = read(&mut client, uri).await;
        assert_eq!(document(&response), json!({ "id": 1, "segment": "posts" }));
        assert_eq!(
            response["result"]["contents"][0]["uri"], uri,
            "echoed as sent"
        );
    }
    let page = read(&mut client, "CRATESTACK://blog/notes?limit=2").await;
    assert_eq!(ids(&document(&page)), [1, 2]);

    let missing = read(&mut client, "cratestack://blog/posts/99999").await;
    for uri in ["cratestack://BLOG/posts/1", "CRATESTACK://Blog/posts/1"] {
        let response = read(&mut client, uri).await;
        assert_eq!(response["error"], missing["error"], "{uri}: {response}");
    }
    client.close().await.unwrap();
}

#[tokio::test]
async fn hidden_missing_and_unknown_are_the_same_error() {
    let mut client = client(FakeResources::default());
    let hidden = read(&mut client, "cratestack://blog/posts/3").await;
    let missing = read(&mut client, "cratestack://blog/posts/99999").await;
    let unparsable = read(&mut client, "cratestack://blog/posts/abc").await;
    let unknown = read(&mut client, "cratestack://blog/Post/1").await;
    assert_eq!(hidden["error"]["code"], json!(-32602), "{hidden}");
    for other in [&missing, &unparsable, &unknown] {
        assert_eq!(hidden["error"], other["error"], "must not differ: {other}");
    }
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_collection_defaults_to_fifty_and_clamps_larger_requests() {
    let mut client = client(FakeResources::default());
    let default = document(&read(&mut client, "cratestack://blog/posts").await);
    assert_eq!(ids(&default), visible_ids()[..50]);
    assert!(default["nextCursor"].is_string());

    let asked_500 = document(&read(&mut client, "cratestack://blog/posts?limit=500").await);
    assert_eq!(ids(&asked_500).len(), 200, "clamped to 200, not refused");

    let notes_500 = document(&read(&mut client, "cratestack://blog/notes?limit=500").await);
    assert_eq!(ids(&notes_500).len(), 20, "max_page_size: 20");
    let notes_default = document(&read(&mut client, "cratestack://blog/notes").await);
    assert_eq!(
        ids(&notes_default).len(),
        20,
        "the default never exceeds it"
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn cursors_walk_exactly_the_visible_rows() {
    let mut client = client(FakeResources::default());
    let mut uri = "cratestack://blog/posts?limit=200".to_owned();
    let (mut seen, mut sizes) = (Vec::new(), Vec::new());
    loop {
        let page = document(&read(&mut client, &uri).await);
        sizes.push(ids(&page).len());
        seen.extend(ids(&page));
        match page["nextCursor"].as_str() {
            Some(next) => uri = format!("cratestack://blog/posts?limit=200&cursor={next}"),
            None => break,
        }
    }
    assert_eq!(
        seen,
        visible_ids(),
        "no hidden row, none skipped, none twice"
    );
    assert_eq!(sizes, [200, 100]);
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_tampered_or_foreign_cursor_is_invalid_params_and_reads_nothing() {
    let table = FakeResources::default();
    let mut client = client(table.clone());
    let first = document(&read(&mut client, "cratestack://blog/posts").await);
    let cursor = first["nextCursor"].as_str().unwrap().to_owned();
    let notes = document(&read(&mut client, "cratestack://blog/notes").await);
    let foreign = notes["nextCursor"].as_str().unwrap().to_owned();
    let reads = table.reads();

    let mut tampered = cursor.clone().into_bytes();
    tampered[5] = if tampered[5] == b'0' { b'1' } else { b'0' };
    let tampered = String::from_utf8(tampered).unwrap();
    for bad in [tampered.as_str(), foreign.as_str(), "garbage"] {
        let response = read(
            &mut client,
            &format!("cratestack://blog/posts?cursor={bad}"),
        )
        .await;
        assert_eq!(
            response["error"]["code"],
            json!(-32602),
            "{bad}: {response}"
        );
    }
    assert_eq!(
        table.reads(),
        reads,
        "a refused cursor never reaches the table"
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn resource_reads_pass_rate_limit_admission() {
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

    let allowed = read(&mut client, "cratestack://blog/posts/1").await;
    assert!(allowed.get("result").is_some(), "{allowed}");
    let throttled = read(&mut client, "cratestack://blog/posts").await;
    assert_eq!(throttled["error"]["code"], json!(-32603), "{throttled}");
    assert_eq!(throttled["error"]["data"]["code"], "TOO_MANY_REQUESTS");
    assert_eq!(table.reads(), 1, "the throttled read never ran");
    assert_eq!(*limiter.keys.lock().unwrap(), ["mcp:u-1", "mcp:u-1"]);
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_tools_only_table_serves_no_resources() {
    let server = StdioServer::new(FakeTools::default(), user("u-1")).unwrap();
    let mut client = Client::start(server);
    let list = client.request("resources/list", json!({})).await;
    assert_eq!(list["result"]["resources"], json!([]));
    let response = read(&mut client, "cratestack://blog/posts/1").await;
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    client.close().await.unwrap();
}

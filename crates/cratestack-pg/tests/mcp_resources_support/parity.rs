//! Visibility parity: the same caller and rows over REST and MCP, each held
//! to the seed rule as well as to the other. Holding MCP to REST alone
//! would not catch a loosened `@@allow` — both would loosen together — so
//! every collection assertion also names the exact visible set.

use cratestack::axum::http::StatusCode;
use serde_json::json;

use super::harness::{Mcp, POSTS, document, rest_get, u1_may_read, u1_visible_posts, user};
use crate::cratestack_schema::Cratestack;

pub async fn listings_name_segments_never_tables(db: &Cratestack) {
    let mut mcp = Mcp::start(db, user("u-1"));
    let resources = mcp.request("resources/list", json!({})).await;
    let uris: Vec<&str> = resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|resource| resource["uri"].as_str().unwrap())
        .collect();
    assert_eq!(uris, ["cratestack://blog/posts", "cratestack://blog/notes"]);

    let templates = mcp.request("resources/templates/list", json!({})).await;
    let templates: Vec<&str> = templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|template| template["uriTemplate"].as_str().unwrap())
        .collect();
    assert!(
        templates.contains(&"cratestack://blog/posts/{id}"),
        "{templates:?}"
    );
    assert!(
        templates.contains(&"cratestack://blog/notes/{id}"),
        "{templates:?}"
    );
    for text in uris.iter().chain(&templates) {
        for leaked in ["McpRes", "mcp_res", "Post", "Note"] {
            assert!(!text.contains(leaked), "`{leaked}` in {text}");
        }
    }
}

pub async fn a_record_reads_like_rest_and_a_hidden_one_like_a_missing_one(db: &Cratestack) {
    let mut mcp = Mcp::start(db, user("u-1"));
    // 1 is published, 6 is u-1's own draft, 3 and 9 are u-2's drafts
    // (hidden), 9999 never existed.
    for id in [1, 2, 3, 4, 6, 9, 12, 255, 256, 9999] {
        let (status, rest) = rest_get(db, "u-1", &format!("/mcp_res_posts/{id}")).await;
        let response = mcp.read(&format!("cratestack://blog/posts/{id}")).await;
        let expected = (1..=POSTS).contains(&id) && u1_may_read(id);
        // MCP is asserted before REST in both branches, so a loosened
        // policy fails on the MCP assertion, not on REST's.
        if expected {
            let record = document(&response);
            assert_eq!(status, StatusCode::OK, "REST {id}: {rest}");
            assert_eq!(
                record, rest,
                "MCP must be shaped exactly like REST's read of {id}"
            );
            assert_eq!(
                record["excerpt"], "post num",
                "`@computed` resolved: {record}"
            );
            assert!(
                record.get("secretNote").is_none(),
                "`@server_only` leaked: {record}"
            );
            assert!(!response.to_string().contains("do not leak"), "{response}");
            assert_eq!(response["result"]["cacheScope"], "private");
            assert_eq!(response["result"]["ttlMs"], json!(0));
        } else {
            assert!(
                response.get("result").is_none(),
                "MCP revealed {id}: {response}"
            );
            assert_eq!(response["error"]["code"], json!(-32602), "{response}");
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "REST must hide {id} too: {rest}"
            );
        }
    }

    // Hidden (exists, u-2's draft) and missing are the same error, byte for byte.
    let hidden = mcp.read("cratestack://blog/posts/3").await;
    let missing = mcp.read("cratestack://blog/posts/9999").await;
    assert_eq!(hidden["error"], missing["error"], "an existence oracle");
    assert_eq!(hidden["error"]["message"], "resource not found");

    // Readable by its owner: the row is really there, only hidden from u-1.
    let mut owner = Mcp::start(db, user("u-2"));
    let as_owner = document(&owner.read("cratestack://blog/posts/3").await);
    assert_eq!(as_owner["id"], 3);
}

pub async fn a_collection_is_exactly_the_rows_rest_lists(db: &Cratestack) {
    let expected = u1_visible_posts();
    assert_eq!(expected.len(), 217, "the seed rule hides 43 of 260");

    let (status, rest) = rest_get(db, "u-1", "/mcp_res_posts?limit=1000&sort=id").await;
    assert_eq!(status, StatusCode::OK, "{rest}");
    let rest_rows = rest
        .as_array()
        .expect("an unpaged model lists an array")
        .clone();
    let rest_ids: Vec<i64> = rest_rows
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect();

    let mut mcp = Mcp::start(db, user("u-1"));
    let mut uri = "cratestack://blog/posts?limit=200".to_owned();
    let mut mcp_rows = Vec::new();
    loop {
        let page = document(&mcp.read(&uri).await);
        mcp_rows.extend(page["items"].as_array().unwrap().iter().cloned());
        match page["nextCursor"].as_str() {
            Some(next) => uri = format!("cratestack://blog/posts?limit=200&cursor={next}"),
            None => break,
        }
    }
    let mcp_ids: Vec<i64> = mcp_rows
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect();
    assert_eq!(
        mcp_ids, expected,
        "MCP pages through exactly the rows u-1 may read"
    );
    assert_eq!(
        rest_ids, expected,
        "REST lists exactly the rows u-1 may read"
    );
    assert_eq!(
        mcp_rows, rest_rows,
        "row for row, field for field, as REST lists them"
    );
    for row in &mcp_rows {
        assert!(
            row.get("secretNote").is_none(),
            "`@server_only` leaked: {row}"
        );
    }

    // Notes: u-1 owns 30 of 35.
    let notes = document(&mcp.read("cratestack://blog/notes?limit=20").await);
    let first: Vec<i64> = item_ids_str(&notes);
    assert_eq!(first, (1..=20).collect::<Vec<_>>());
}

/// Note ids are `n-001`-style strings; their number, for comparison.
fn item_ids_str(page: &serde_json::Value) -> Vec<i64> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap()[2..].parse().unwrap())
        .collect()
}

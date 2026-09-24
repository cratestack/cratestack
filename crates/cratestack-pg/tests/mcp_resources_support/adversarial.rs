//! What an agent can type that REST's router would never route, and what a
//! hidden row may never influence (cratestack#1040 review). Every refused
//! read here must be the *same* error as a plainly missing id: an answer
//! that differs by row is an existence oracle (security requirement 12), and
//! one that differs by spelling only teaches the agent which spellings the
//! parser reached.

use cratestack::CratestackContext;
use serde_json::{Value, json};

use super::harness::{Mcp, NEVER_SENT, document, item_ids, user};
use crate::cratestack_schema::Cratestack;

fn assert_nothing_hidden(response: &Value) {
    let text = response.to_string();
    for leaked in NEVER_SENT {
        assert!(!text.contains(leaked), "`{leaked}` reached MCP: {text}");
    }
}

pub async fn odd_uris_answer_exactly_like_a_missing_row(db: &Cratestack) {
    let mut mcp = Mcp::start(db, user("u-1"));
    let missing = mcp.read("cratestack://blog/posts/9999").await;
    assert_eq!(missing["error"]["message"], "resource not found");
    for uri in [
        // Ids that are not an `Int` key, or name a hidden row another way.
        "cratestack://blog/posts/abc",
        "cratestack://blog/posts/-3",
        "cratestack://blog/posts/+3",
        "cratestack://blog/posts/0",
        "cratestack://blog/posts/99999999999999999999",
        "cratestack://blog/posts/1.0",
        "cratestack://blog/posts/1%2F2",
        "cratestack://blog/posts/..%2F1",
        "cratestack://blog/posts/3%3Flimit%3D1",
        "cratestack://blog/posts/%201",
        "cratestack://blog/posts/3%00",
        // Not a resource: an unannotated model by any name, another
        // schema, and the shapes around a real record URI.
        "cratestack://blog/McpResAuthor/u-1",
        "cratestack://blog/mcp_res_authors/u-1",
        "cratestack://blog/authors/u-1",
        "cratestack://blog/McpResPost/1",
        "cratestack://blog/mcp_res_posts/1",
        "cratestack://other/posts/1",
        "cratestack://blog/posts/",
        "cratestack://blog/posts/1/",
        "cratestack://blog/posts/1/author",
        "CRATESTACK://blog/posts/1",
    ] {
        let response = mcp.read(uri).await;
        assert_eq!(response["error"], missing["error"], "{uri}: {response}");
    }

    // String keys: a hidden note (u-2's) and a missing one, with the same
    // id tricks, all answer as missing.
    let missing_note = mcp.read("cratestack://blog/notes/n-999").await;
    assert_eq!(missing_note["error"], missing["error"]);
    for uri in [
        "cratestack://blog/notes/n-031",
        "cratestack://blog/notes/n%2D031",
        "cratestack://blog/notes/N-031",
        "cratestack://blog/notes/n-031%20",
    ] {
        let response = mcp.read(uri).await;
        assert_eq!(response["error"], missing["error"], "{uri}: {response}");
    }

    // A NUL in a text key is refused by Postgres itself, before any row is
    // compared, so it cannot be "not found" — but it must not depend on
    // whether the row exists either: visible, hidden and missing ids fail
    // alike, and name nothing.
    let nul = mcp.read("cratestack://blog/notes/n-001%00").await;
    println!("NUL in a text key: {nul}");
    for uri in [
        "cratestack://blog/notes/n-031%00",
        "cratestack://blog/notes/n-999%00",
    ] {
        let response = mcp.read(uri).await;
        assert_eq!(response["error"], nul["error"], "{uri}: {response}");
    }
    assert!(nul.get("result").is_none(), "{nul}");
    assert!(!nul.to_string().contains("mcp_res"), "{nul}");
}

/// REST's default read includes no relation, so neither may a resource: a
/// visible post by a hidden author names only `authorId`, a column of the
/// post itself, and nothing of the author row or its `@server_only` field.
pub async fn a_record_carries_no_relation(db: &Cratestack) {
    let mut mcp = Mcp::start(db, user("u-1"));
    // Post 1 is published and u-2's; u-2's author row is hidden from u-1.
    let response = mcp.read("cratestack://blog/posts/1").await;
    let record = document(&response);
    assert_eq!(record["authorId"], "u-2", "{record}");
    assert!(
        record.get("author").is_none(),
        "a relation leaked: {record}"
    );
    assert_nothing_hidden(&response);

    let page = mcp.read("cratestack://blog/posts?limit=200").await;
    for item in document(&page)["items"].as_array().unwrap() {
        assert!(item.get("author").is_none(), "a relation leaked: {item}");
    }
    assert_nothing_hidden(&page);
}

/// Where hidden rows sit changes nothing a caller can count: u-1 owns 30 of
/// 35 notes, and the five it may not read sort last. Its second page of 20
/// is the last 10 — with no `nextCursor` promising the hidden five.
pub async fn hidden_rows_promise_no_further_page(db: &Cratestack) {
    let mut mcp = Mcp::start(db, user("u-1"));
    let first = document(&mcp.read("cratestack://blog/notes?limit=20").await);
    let next = first["nextCursor"]
        .as_str()
        .expect("ten more visible notes");
    let second = document(
        &mcp.read(&format!("cratestack://blog/notes?limit=20&cursor={next}"))
            .await,
    );
    assert_eq!(second["items"].as_array().unwrap().len(), 10, "{second}");
    assert!(second.get("nextCursor").is_none(), "{second}");

    let mut anonymous = Mcp::start(db, CratestackContext::anonymous());
    let response = anonymous.read("cratestack://blog/notes").await;
    let contents = response["result"]["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1, "never an empty `contents`: {response}");
    assert_eq!(document(&response), json!({ "items": [] }));
    let posts = document(&anonymous.read("cratestack://blog/posts?limit=200").await);
    assert!(
        item_ids(&posts).iter().all(|id| id % 3 != 0),
        "an anonymous caller reads published posts only: {posts}"
    );
    assert_nothing_hidden(&posts);
}

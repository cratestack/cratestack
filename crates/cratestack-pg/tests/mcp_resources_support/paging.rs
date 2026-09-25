//! Collection paging against real rows: Q3's sizes, the cursor, and the
//! SQL a page actually runs, read from sqlx's statement log rather than
//! from a preview, so what is quoted is what executed.

use cratestack::mcp::McpTools;
use serde_json::json;

use super::harness::{Mcp, document, item_ids, table, u1_visible_posts, user};
use crate::cratestack_schema::Cratestack;
use crate::support::tracing_capture::capture_events;

pub async fn page_sizes_follow_q3(db: &Cratestack) {
    let expected = u1_visible_posts();
    let mut mcp = Mcp::start(db, user("u-1"));

    let default = document(&mcp.read("cratestack://blog/posts").await);
    assert_eq!(item_ids(&default), expected[..50], "50 by default");

    let asked_500 = document(&mcp.read("cratestack://blog/posts?limit=500").await);
    assert_eq!(
        item_ids(&asked_500),
        expected[..200],
        "500 is clamped to 200, not refused"
    );

    // The page after 200 visible rows starts at the 201st *visible* row:
    // the 43 hidden rows interleaved before it moved no boundary.
    let next = asked_500["nextCursor"].as_str().expect("more rows");
    let rest = document(
        &mcp.read(&format!("cratestack://blog/posts?limit=500&cursor={next}"))
            .await,
    );
    assert_eq!(item_ids(&rest), expected[200..]);
    assert!(rest.get("nextCursor").is_none(), "the last page: {rest}");

    for (uri, count) in [
        ("cratestack://blog/notes", 20),
        ("cratestack://blog/notes?limit=500", 20),
        ("cratestack://blog/notes?limit=5", 5),
    ] {
        let page = document(&mcp.read(uri).await);
        let items = page["items"].as_array().unwrap();
        assert_eq!(items.len(), count, "{uri}: `max_page_size: 20` caps it");
    }
}

pub async fn a_tampered_cursor_is_invalid_params(db: &Cratestack) {
    let mut mcp = Mcp::start(db, user("u-1"));
    let posts = document(&mcp.read("cratestack://blog/posts").await);
    let cursor = posts["nextCursor"].as_str().unwrap().to_owned();
    let notes = document(&mcp.read("cratestack://blog/notes?limit=5").await);
    let foreign = notes["nextCursor"].as_str().unwrap().to_owned();

    let mut tampered = cursor.into_bytes();
    let last = tampered.len() - 1;
    tampered[last] = if tampered[last] == b'0' { b'1' } else { b'0' };
    let tampered = String::from_utf8(tampered).unwrap();
    for bad in [tampered.as_str(), foreign.as_str(), "0", "not-a-cursor"] {
        let response = mcp
            .read(&format!("cratestack://blog/posts?cursor={bad}"))
            .await;
        assert_eq!(
            response["error"]["code"],
            json!(-32602),
            "{bad}: {response}"
        );
    }
}

/// Runs the generated `read_page` itself — what `resources/read` calls —
/// under a statement capture, and pins the shape the no-oracle argument
/// rests on: the policy is in the `WHERE`, and `ORDER BY`, `LIMIT` and
/// `OFFSET` come after it, so an offset counts visible rows only.
pub async fn the_page_query_filters_before_it_limits(db: &Cratestack) {
    let tools = table(db);
    let ctx = user("u-1");
    let (rows, events) = capture_events(tools.read_page("posts", 51, 50, &ctx)).await;
    let ids: Vec<i64> = rows
        .expect("a page")
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, u1_visible_posts()[50..101]);

    let statements: Vec<&String> = events
        .iter()
        .filter(|line| line.contains("mcp_res_posts") && line.contains("LIMIT"))
        .collect();
    let [statement] = statements.as_slice() else {
        panic!("expected exactly one page statement: {events:#?}");
    };
    println!("MCP collection page SQL: {statement}");
    let at = |needle: &str| {
        statement
            .find(needle)
            .unwrap_or_else(|| panic!("no `{needle}` in {statement}"))
    };
    // The compiled `published || authorId == auth().id`, with the
    // caller's id bound — not the projection's `published AS ...`.
    let policy = "(published = TRUE OR author_id = $1)";
    assert!(at("WHERE") < at(policy), "{statement}");
    assert!(at(policy) < at("ORDER BY"), "{statement}");
    assert!(at("ORDER BY") < at("LIMIT"), "{statement}");
    assert!(at("LIMIT") < at("OFFSET"), "{statement}");
}

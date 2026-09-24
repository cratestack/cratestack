//! cratestack#1040: a `db = None` schema serves no MCP resources. The
//! parser already rejects `@@mcp(resource: ...)` there (no models can
//! exist); this is the runtime half, on the generated table itself and on
//! the wire, so a later change that emitted resource methods for a
//! database-less schema would be caught here, not by an agent.
//!
//! Gated `required-features = ["mcp"]`; `just test-ci-host` runs it.

mod mcp_support;

use cratestack::mcp::{McpTools, StdioServer};
use mcp_support::client::Client;
use mcp_support::{Registry, caller, tools};
use serde_json::json;

#[tokio::test]
async fn a_database_less_schema_exposes_no_resources() {
    let registry = Registry::default();
    let table = tools(&registry);
    assert!(table.resources().is_empty(), "the generated table lists none");
    let ctx = caller("u-1", "teller");
    assert_eq!(
        table.read_record("anything", "1", &ctx).await.unwrap(),
        None,
        "and reads none"
    );

    let mut client = Client::start(StdioServer::new(table, ctx).expect("valid table"));
    let discovered = client.request("server/discover", json!({})).await;
    assert!(
        discovered["result"]["capabilities"].get("resources").is_none(),
        "no resources capability: {discovered}"
    );
    let listed = client.request("resources/list", json!({})).await;
    assert_eq!(listed["result"]["resources"], json!([]));
    let templates = client.request("resources/templates/list", json!({})).await;
    assert_eq!(templates["result"]["resourceTemplates"], json!([]));
    let read = client
        .request("resources/read", json!({ "uri": "cratestack://mcp_tools/anything/1" }))
        .await;
    assert_eq!(read["error"]["code"], json!(-32602), "{read}");
}

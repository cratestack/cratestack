//! The wire contract of the stdio server (cratestack#1038), driven as raw
//! JSON-RPC over an in-memory duplex: discovery, listing, the protocol-
//! version pin, and how each failure class is reported.

mod support;

use cratestack_core::SystemContext;
use cratestack_mcp::{PROTOCOL_VERSION, StdioServer};
use serde_json::{Value, json};
use support::client::{Client, envelope, text};
use support::{ECHO_INPUT, ECHO_OUTPUT, FakeTools, TRANSFER_INPUT};

fn server(tools: FakeTools) -> StdioServer<FakeTools> {
    StdioServer::new(tools, SystemContext::for_service("tests").into_context())
        .expect("the fake table is valid")
}

#[tokio::test]
async fn discover_advertises_exactly_2026_07_28_and_tools() {
    let mut client = Client::start(server(FakeTools::default()));
    let response = client.request("server/discover", json!({})).await;
    let result = &response["result"];
    assert_eq!(result["supportedVersions"], json!([PROTOCOL_VERSION]));
    assert_eq!(PROTOCOL_VERSION, "2026-07-28");
    assert!(result["capabilities"]["tools"].is_object(), "{result}");
    assert!(result["capabilities"].get("resources").is_none());
    client.close().await.expect("clean exit");
}

#[tokio::test]
async fn tools_list_is_the_table_in_order_with_schemas_and_hints() {
    let mut client = Client::start(server(FakeTools::default()));
    let response = client.request("tools/list", json!({})).await;
    let tools = response["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["echo", "transfer"], "declaration order, nothing else");

    let parse = |s: &str| serde_json::from_str::<Value>(s).unwrap();
    assert_eq!(tools[0]["inputSchema"], parse(ECHO_INPUT));
    assert_eq!(tools[0]["outputSchema"], parse(ECHO_OUTPUT));
    assert_eq!(tools[0]["description"], "Echo the text back.");
    assert_eq!(tools[0]["annotations"], json!({ "readOnlyHint": true }));

    assert_eq!(tools[1]["inputSchema"], parse(TRANSFER_INPUT));
    assert!(tools[1].get("outputSchema").is_none(), "a list output has none");
    assert!(tools[1].get("description").is_none(), "no description was declared");
    assert_eq!(
        tools[1]["annotations"],
        json!({ "readOnlyHint": false, "idempotentHint": false }),
        "a mutation that takes reservations is not idempotent"
    );
    client.close().await.expect("clean exit");
}

#[tokio::test]
async fn an_unknown_tool_is_invalid_params() {
    let tools = FakeTools::default();
    let mut client = Client::start(server(tools.clone()));
    let response = client
        .request("tools/call", json!({ "name": "nope", "arguments": {} }))
        .await;
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    assert!(response.get("result").is_none());
    assert_eq!(tools.runs(), 0);
    client.close().await.expect("clean exit");
}

#[tokio::test]
async fn bad_arguments_are_an_is_error_result_naming_the_field() {
    let tools = FakeTools::default();
    let mut client = Client::start(server(tools.clone()));

    let wrong_type = client.call("transfer", json!({ "amount": "ten" }), None).await;
    let error = envelope(&wrong_type);
    assert_eq!(error["code"], "VALIDATION_ERROR");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("`amount`"), "names the field: {message}");

    let missing = client.call("echo", json!({}), None).await;
    let message = envelope(&missing)["message"].as_str().unwrap().to_owned();
    assert!(message.contains("`text`"), "names the missing field: {message}");

    assert_eq!(tools.runs(), 0, "nothing ran");
    client.close().await.expect("clean exit");
}

#[tokio::test]
async fn success_is_structured_content_and_text() {
    let mut client = Client::start(server(FakeTools::default()));
    let result = client.call("echo", json!({ "text": "hi" }), None).await;
    assert_eq!(result["isError"], json!(false));
    assert_eq!(result["structuredContent"], json!({ "text": "hi" }));
    assert_eq!(
        serde_json::from_str::<Value>(text(&result)).unwrap(),
        json!({ "text": "hi" }),
        "the same value, again as a text block"
    );

    let list = client.call("transfer", json!({ "amount": 3 }), None).await;
    assert!(list.get("structuredContent").is_none(), "no outputSchema, no structuredContent");
    // Run 2: the echo above was run 1.
    assert_eq!(text(&list), r#"[3,2,"system:tests"]"#);
    client.close().await.expect("clean exit");
}

#[tokio::test]
async fn a_5xx_error_reveals_only_the_rest_envelope() {
    let mut client = Client::start(server(FakeTools::default()));
    let result = client.call("transfer", json!({ "amount": -1 }), None).await;
    assert_eq!(
        envelope(&result),
        json!({ "code": "DATABASE_ERROR", "message": "internal error", "details": null }),
        "the operator detail stays in the log"
    );
    assert!(!text(&result).contains("ledger"));
    client.close().await.expect("clean exit");
}

#[tokio::test]
async fn another_protocol_version_is_refused() {
    let tools = FakeTools::default();
    let mut client = Client::start(server(tools.clone()));
    // A 2026-07-28-shaped request that names the previous revision.
    let response = client
        .request_as(
            "2025-11-25",
            "tools/call",
            json!({ "name": "echo", "arguments": { "text": "x" } }),
        )
        .await;
    assert!(response.get("error").is_some(), "{response}");
    assert_eq!(tools.runs(), 0);
    client.close().await.expect("clean exit");
}

#[tokio::test]
async fn a_legacy_initialize_is_refused() {
    let mut client = Client::start(server(FakeTools::default()));
    let response = client
        .send(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "legacy", "version": "0" },
            },
        }))
        .await;
    assert!(
        response.get("error").is_some(),
        "no handshake revision is supported, so initialize cannot succeed: {response}"
    );
    // `rmcp` ends a connection whose opening request failed.
    assert!(client.close().await.is_err());
}

#[tokio::test]
async fn closing_input_before_any_request_is_a_clean_exit() {
    let client = Client::start(server(FakeTools::default()));
    client.close().await.expect("nothing to serve is not an error");
}

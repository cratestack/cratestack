//! A resource id carrying a raw character a URI may not (maintainer
//! decision on #1040) is answered over the wire exactly like a missing row,
//! and never reaches the table; its percent-encoded form reads the row.
//!
//! The table here holds a row under *every* id, echoing the id it was
//! asked for, so a raw id the URI layer let through would come back as a
//! record instead of the error. A table of ordinary keys could not tell
//! the two apart: `a b` is missing from it whether or not it was refused.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::Body;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use cratestack_core::{CratestackContext, CratestackError, OpDescriptor, OpKind};
use cratestack_mcp::{ArgumentsError, McpTools, ResourceDescriptor, StdioServer, ToolDescriptor};
use serde_json::{Value, json};
use support::client::Client;
use support::http_app::{Reply, post, rpc, send, served, token};
use support::user;

static GET: OpDescriptor = OpDescriptor {
    op_id: "model.Note.get",
    kind: OpKind::Unary,
    input_ty: "",
    output_ty: "",
    idempotent_by_default: true,
    rate_limited_by_default: true,
    auth_required: false,
};
static RESOURCES: [ResourceDescriptor; 1] =
    [ResourceDescriptor::new("blog", "notes", 20, &GET, &GET)];

/// The one id with no row.
const MISSING: &str = "missing";

#[derive(Clone, Default)]
struct EchoTable {
    reads: Arc<AtomicUsize>,
}

enum NoCall {}

impl McpTools for EchoTable {
    type Call = NoCall;

    fn tools(&self) -> &'static [ToolDescriptor] {
        &[]
    }

    fn decode(&self, tool: &str, _: Value) -> Result<NoCall, ArgumentsError> {
        Err(ArgumentsError::new(format!("no tool `{tool}`")))
    }

    async fn execute(&self, call: NoCall, _: &CratestackContext) -> Result<Value, CratestackError> {
        match call {}
    }

    fn resources(&self) -> &'static [ResourceDescriptor] {
        &RESOURCES
    }

    async fn read_record(
        &self,
        _segment: &str,
        id: &str,
        _ctx: &CratestackContext,
    ) -> Result<Option<Value>, CratestackError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok((id != MISSING).then(|| json!({ "id": id })))
    }
}

async fn read(client: &mut Client, id: &str) -> Value {
    let uri = format!("cratestack://blog/notes/{id}");
    client
        .request("resources/read", json!({ "uri": uri }))
        .await
}

#[tokio::test]
async fn a_raw_non_uri_character_is_the_same_error_as_a_missing_row() {
    let table = EchoTable::default();
    let mut client = Client::start(StdioServer::new(table.clone(), user("u-1")).unwrap());
    let missing = read(&mut client, MISSING).await;
    assert_eq!(
        missing["error"]["message"], "resource not found",
        "{missing}"
    );
    assert_eq!(table.reads.load(Ordering::SeqCst), 1);

    for id in [
        "a b", "a\tb", "\"a\"", "<a>", "a\\b", "a{b}", "a|b", "a^b", "a`b", "[a]", "café",
    ] {
        let response = read(&mut client, id).await;
        assert_eq!(response["error"], missing["error"], "{id:?}: {response}");
    }
    assert_eq!(
        table.reads.load(Ordering::SeqCst),
        1,
        "a refused id never reaches the table"
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_percent_encoded_id_reads_the_decoded_row() {
    let mut client = Client::start(StdioServer::new(EchoTable::default(), user("u-1")).unwrap());
    for (encoded, decoded) in [
        ("a%20b", "a b"),
        ("%22a%22", "\"a\""),
        ("%3Ca%3E", "<a>"),
        ("caf%C3%A9", "café"),
        ("n-001:@!$&'()*+,;=._~", "n-001:@!$&'()*+,;=._~"),
    ] {
        let response = read(&mut client, encoded).await;
        let contents = &response["result"]["contents"];
        let text = contents[0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{encoded}: {response}"));
        let record: Value = serde_json::from_str(text).unwrap();
        assert_eq!(record, json!({ "id": decoded }), "{encoded}");
    }
    client.close().await.unwrap();
}

/// A `resources/read` over Streamable HTTP whose `Mcp-Name` is `name`.
async fn http_read(app: &axum::Router, uri: &str, name: &str) -> Reply {
    let body = rpc("resources/read", json!({ "uri": uri }));
    let request = post("/mcp", Some(&token("u-1")), &body)
        .header("mcp-name", name)
        .body(Body::from(body.to_string()))
        .unwrap();
    send(app, request).await
}

/// Over Streamable HTTP, `Mcp-Name` must mirror the URI (SEP-2243), and a
/// value no header can carry raw travels as `=?base64?...?=`, which is how
/// `rmcp`'s own client sends it. So a raw control character is not stopped
/// by the header layer: a conforming client's request reaches the handler
/// and must get the missing row's answer there, byte for byte, without a
/// read. A header that does not mirror the body is refused before the
/// handler, also without a read.
#[tokio::test]
async fn over_http_a_raw_id_is_not_found_and_never_reaches_the_table() {
    let table = EchoTable::default();
    let app = served(&table);
    let base = "cratestack://blog/notes";
    let missing_uri = format!("{base}/{MISSING}");
    let missing = http_read(&app, &missing_uri, &missing_uri).await;
    assert_eq!(missing.json()["error"]["message"], "resource not found");
    assert_eq!(table.reads.load(Ordering::SeqCst), 1);

    for id in [
        "a b", "a\nb", "a\rb", "\0", "a\u{1}b", "a\u{7f}b", "café", "a\"b",
    ] {
        let uri = format!("{base}/{id}");
        let name = format!("=?base64?{}?=", BASE64_STANDARD.encode(&uri));
        let reply = http_read(&app, &uri, &name).await;
        assert_eq!(reply.status, missing.status, "{id:?}: {}", reply.text);
        assert_eq!(reply.json()["error"], missing.json()["error"], "{id:?}");
    }
    let raw = format!("{base}/a b");
    let plain = http_read(&app, &raw, &raw).await;
    assert_eq!(
        plain.json()["error"],
        missing.json()["error"],
        "{}",
        plain.text
    );

    let mismatched = http_read(&app, &format!("{base}/a\nb"), &format!("{base}/ab")).await;
    assert_eq!(mismatched.status, 400, "{}", mismatched.text);
    assert_eq!(
        table.reads.load(Ordering::SeqCst),
        1,
        "a refused id never reaches the table"
    );
}

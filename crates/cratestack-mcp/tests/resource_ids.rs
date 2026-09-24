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

use cratestack_core::{CratestackContext, CratestackError, OpDescriptor, OpKind};
use cratestack_mcp::{ArgumentsError, McpTools, ResourceDescriptor, StdioServer, ToolDescriptor};
use serde_json::{Value, json};
use support::client::Client;
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

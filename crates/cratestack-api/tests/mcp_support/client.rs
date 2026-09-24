//! A raw JSON-RPC client over an in-memory duplex with stdio's
//! newline-delimited framing: the bytes an MCP client would see, not an
//! SDK's decoding of them. (`cratestack-mcp`'s own tests carry a twin; a
//! test helper cannot be shared across crates without publishing it.)

use cratestack::mcp::{McpTools, StdioServer};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

pub struct Client {
    writer: DuplexStream,
    reader: Lines<BufReader<DuplexStream>>,
    next_id: u64,
}

impl Client {
    pub fn start<T: McpTools>(server: StdioServer<T>) -> Self {
        let (writer, server_reader) = tokio::io::duplex(1 << 16);
        let (server_writer, reader) = tokio::io::duplex(1 << 16);
        tokio::spawn(server.serve_io(server_reader, server_writer));
        Self {
            writer,
            reader: BufReader::new(reader).lines(),
            next_id: 1,
        }
    }

    /// One request with the per-request `_meta` 2026-07-28 requires, merged
    /// with any `_meta` already in `params`. Returns the whole response.
    pub async fn request(&mut self, method: &str, mut params: Value) -> Value {
        let mut meta = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        if let Some(Value::Object(extra)) = params.as_object_mut().unwrap().remove("_meta") {
            meta.as_object_mut().unwrap().extend(extra);
        }
        params["_meta"] = meta;
        let message =
            json!({ "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params });
        self.next_id += 1;
        let mut line = message.to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
        let reply =
            tokio::time::timeout(std::time::Duration::from_secs(10), self.reader.next_line())
                .await
                .expect("answered within 10s")
                .unwrap()
                .expect("the server answered");
        serde_json::from_str(&reply).expect("JSON on the wire")
    }

    /// `tools/call`; the `result` object (panics on a JSON-RPC error).
    pub async fn call(&mut self, name: &str, arguments: Value, meta: Option<Value>) -> Value {
        let mut params = json!({ "name": name, "arguments": arguments });
        if let Some(meta) = meta {
            params["_meta"] = meta;
        }
        let response = self.request("tools/call", params).await;
        response
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("tools/call `{name}` was a protocol error: {response}"))
    }
}

/// The REST error envelope an `isError` result carries as its text.
pub fn envelope(result: &Value) -> Value {
    assert_eq!(
        result["isError"],
        json!(true),
        "expected an isError result: {result}"
    );
    let text = result["content"][0]["text"].as_str().expect("a text block");
    serde_json::from_str(text).expect("the REST envelope")
}

/// `_meta` carrying an idempotency key (ADR 0002 Q6).
pub fn key(value: &str) -> Option<Value> {
    Some(json!({ "dev.cratestack/idempotencyKey": value }))
}

//! A raw JSON-RPC client over an in-memory duplex, speaking stdio's
//! newline-delimited framing. Raw on purpose: the assertions are about the
//! bytes a real client sees (error codes, `isError`, the version list), and
//! an SDK client would decode some of that away.

use cratestack_mcp::{ServeError, StdioServer};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};
use tokio::task::JoinHandle;

use super::FakeTools;

pub struct Client {
    writer: Option<DuplexStream>,
    reader: Lines<BufReader<DuplexStream>>,
    next_id: u64,
    pub server: Option<JoinHandle<Result<(), ServeError>>>,
}

/// The per-request `_meta` 2026-07-28 requires on every request.
pub fn meta(version: &str) -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": version,
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "test-client", "version": "0" },
    })
}

impl Client {
    pub fn start(server: StdioServer<FakeTools>) -> Self {
        let (client_writer, server_reader) = tokio::io::duplex(1 << 16);
        let (server_writer, client_reader) = tokio::io::duplex(1 << 16);
        let handle = tokio::spawn(server.serve_io(server_reader, server_writer));
        Self {
            writer: Some(client_writer),
            reader: BufReader::new(client_reader).lines(),
            next_id: 1,
            server: Some(handle),
        }
    }

    /// Send one request with valid 2026-07-28 metadata and return the whole
    /// response message.
    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        self.request_as("2026-07-28", method, params).await
    }

    pub async fn request_as(&mut self, version: &str, method: &str, mut params: Value) -> Value {
        let object = params.as_object_mut().expect("params are an object");
        let mut merged = meta(version);
        if let Some(Value::Object(extra)) = object.remove("_meta") {
            merged.as_object_mut().unwrap().extend(extra);
        }
        object.insert("_meta".to_owned(), merged);
        self.send(json!({ "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params }))
            .await
    }

    pub async fn send(&mut self, message: Value) -> Value {
        self.next_id += 1;
        let mut line = message.to_string();
        line.push('\n');
        let writer = self.writer.as_mut().expect("client still open");
        writer.write_all(line.as_bytes()).await.expect("write");
        writer.flush().await.expect("flush");
        let reply = tokio::time::timeout(std::time::Duration::from_secs(10), self.reader.next_line())
            .await
            .expect("server answered within 10s")
            .expect("read")
            .expect("server closed the stream instead of answering");
        serde_json::from_str(&reply).expect("the server writes JSON")
    }

    /// `tools/call`, returning the `result` object (panics on a JSON-RPC error).
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

    /// Close the input side; the server must then stop on its own.
    pub async fn close(mut self) -> Result<(), ServeError> {
        drop(self.writer.take());
        let handle = self.server.take().expect("server handle");
        tokio::time::timeout(std::time::Duration::from_secs(10), handle)
            .await
            .expect("server stopped within 10s of its input closing")
            .expect("server task did not panic")
    }
}

/// The text of a result's first content block.
pub fn text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().expect("a text block")
}

/// The REST-shaped error envelope an `isError` result carries.
pub fn envelope(result: &Value) -> Value {
    assert_eq!(result["isError"], json!(true), "expected an isError result: {result}");
    serde_json::from_str(text(result)).expect("the error text is the REST envelope")
}

//! One request per transport: what the kit sends (its canonical form, for
//! the signer and for the comparison) and what came back.

use cratestack::axum::Router;
use cratestack::axum::body::Body;
use http::header::WWW_AUTHENTICATE;
use http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::canonical::{Canonical, Transport};

const JSON: &str = "application/json";
const TAG: &str = "conformance";

/// The request the kit sends on `transport`, mounted under `mount`.
pub(crate) fn canonical(transport: Transport, mount: &'static str) -> Canonical {
    let input = json!({ "tag": TAG });
    let (path, query, body) = match transport {
        Transport::Rest => ("/$procs/whoAmI", None, input),
        Transport::RpcUnary => ("/rpc/procedure.whoAmI", None, input),
        Transport::RpcBatch => {
            let frame = |id: u64| json!({ "id": id, "op": "procedure.whoAmI", "input": input });
            ("/rpc/batch", None, json!([frame(1), frame(2)]))
        }
        Transport::Mcp => ("/mcp", Some("kit=1"), mcp_call(&input)),
    };
    Canonical {
        transport,
        mount,
        method: "POST",
        path,
        query,
        body: body.to_string().into_bytes(),
    }
}

fn mcp_call(input: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "whoAmI",
            "arguments": input,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/clientInfo": { "name": "conformance", "version": "0" },
            },
        },
    })
}

/// What came back.
pub(crate) struct Answer {
    pub(crate) status: StatusCode,
    pub(crate) challenges: Vec<String>,
    pub(crate) body: Value,
}

/// Sends `canonical` to `app` with the signer's `credential` headers.
pub(crate) async fn send(app: &Router, canonical: &Canonical, credential: HeaderMap) -> Answer {
    // Every transport sends `?kit=1`: the provider sees it only where the
    // operation reads a query (MCP here), and `None` elsewhere.
    let uri = format!("{}{}?kit=1", canonical.mount, canonical.path);
    let mut request = Request::post(uri).header("content-type", JSON);
    if canonical.transport != Transport::Mcp {
        request = request.header("accept", JSON);
    } else {
        request = request
            .header("host", "localhost")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", "tools/call")
            .header("mcp-name", "whoAmI");
    }
    let mut request = request
        .body(Body::from(canonical.body.clone()))
        .expect("the kit builds a valid request");
    for (name, value) in &credential {
        request.headers_mut().append(name, value.clone());
    }
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("an axum router is infallible");
    let status = response.status();
    let challenges = response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .collect();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .unwrap_or_default();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    Answer {
        status,
        challenges,
        body,
    }
}

/// The subjects an accepted call returned: one for REST, RPC unary and MCP,
/// one per frame for a batch. An entry is `Err` with what came back instead.
pub(crate) fn subjects(transport: Transport, answer: &Answer) -> Vec<Result<String, String>> {
    let read = |value: &Value| {
        value["subject"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| value.to_string())
    };
    if answer.status != StatusCode::OK {
        return vec![Err(format!("{} {}", answer.status, answer.body))];
    }
    match transport {
        Transport::RpcBatch => match answer.body.as_array() {
            Some(frames) => frames.iter().map(|frame| read(&frame["output"])).collect(),
            None => vec![Err(answer.body.to_string())],
        },
        Transport::Mcp => {
            let result = &answer.body["result"];
            if result["isError"] == json!(true) {
                vec![Err(result.to_string())]
            } else {
                vec![read(&result["structuredContent"])]
            }
        }
        _ => vec![read(&answer.body)],
    }
}

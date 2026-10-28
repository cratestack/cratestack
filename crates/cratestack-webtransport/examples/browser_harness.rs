//! A WebTransport server for the real-browser end-to-end check
//! (`tests/browser/run.cjs`, `just test-webtransport-browser`). Not a
//! usage example: it serves a scripted dispatcher and prints JSON lines —
//! first `{"port":…,"sha256":[…]}`, then `{"dropped":true}` whenever a
//! client's cancel reaches the dispatcher.
//!
//! Usage: browser_harness <allowed-origin>

use std::io::Write;
use std::time::Duration;

use cratestack_core::{
    CratestackContext, CratestackError, SubscriptionDispatcher, SubscriptionOpen,
    SubscriptionStream, Value,
};
use cratestack_webtransport::{
    DevCertificate, OriginPolicy, WebTransportConfig, WebTransportServer,
};
use futures_util::stream;

fn auth(headers: &http::HeaderMap) -> Result<CratestackContext, CratestackError> {
    match headers
        .get(http::header::AUTHORIZATION)
        .map(|v| v.to_str().unwrap_or_default())
    {
        None => Ok(CratestackContext::anonymous()),
        Some("Bearer bad") => Err(CratestackError::Unauthorized("bad token".into())),
        Some(value) => {
            let id = value.trim_start_matches("Bearer ").to_owned();
            Ok(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::String(id),
            )]))
        }
    }
}

#[derive(Clone)]
struct Scripted;

struct ReportDrop;
impl Drop for ReportDrop {
    fn drop(&mut self) {
        println!("{}", serde_json::json!({ "dropped": true }));
        let _ = std::io::stdout().flush();
    }
}

impl SubscriptionDispatcher for Scripted {
    async fn open(
        &self,
        open: SubscriptionOpen,
    ) -> Result<SubscriptionStream<serde_json::Value>, CratestackError> {
        match open.op_id.as_str() {
            "test.items" => {
                let n = open.input["n"].as_u64().unwrap_or(0);
                Ok(Box::pin(stream::iter(
                    (0..n).map(|i| Ok(serde_json::json!({ "i": i }))),
                )))
            }
            "test.forever" => Ok(Box::pin(stream::unfold(
                (0u64, ReportDrop),
                |(i, guard)| async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Some((Ok(serde_json::json!(i)), (i + 1, guard)))
                },
            ))),
            "test.whoami" => {
                let who = cratestack_webtransport::default_principal_key(&open.ctx);
                Ok(Box::pin(stream::iter([Ok(serde_json::json!(who))])))
            }
            "test.refuse" => Err(CratestackError::Forbidden(
                "not a member of this room".into(),
            )),
            other => Err(CratestackError::NotFound(format!(
                "unknown subscription op `{other}`"
            ))),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), CratestackError> {
    let origin = std::env::args()
        .nth(1)
        .expect("usage: browser_harness <allowed-origin>");
    let cert = DevCertificate::generate(&["localhost", "127.0.0.1"])?;
    let config = WebTransportConfig::new(
        "127.0.0.1:0".parse().unwrap(),
        cert.certificate_chain,
        cert.private_key,
    )
    .origins(OriginPolicy::Allow(vec![origin]));
    let server = WebTransportServer::bind(config, auth, Scripted)?;
    let port = server.local_addr()?.port();
    println!(
        "{}",
        serde_json::json!({ "port": port, "sha256": cert.sha256.to_vec() })
    );
    std::io::stdout().flush().ok();
    server.serve().await;
    Ok(())
}

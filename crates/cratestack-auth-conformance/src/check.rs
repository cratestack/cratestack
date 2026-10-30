//! The assertions: one accepted or refused request on one transport, as a
//! list of findings (empty when it conforms).

use http::StatusCode;
use serde_json::Value;

use crate::canonical::{Canonical, Seen, Transport};
use crate::expect::Expected;
use crate::send::{Answer, subjects};

/// One call per request, and the canonical request exactly (ADR 0021 §3).
pub(crate) fn canonical(canonical: &Canonical, seen: &[Seen], calls: usize) -> Vec<String> {
    if seen.len() != calls {
        return vec![format!(
            "the provider was called {} time(s), expected {calls}",
            seen.len()
        )];
    }
    let expected = Seen::expected(canonical);
    seen.iter()
        .filter(|seen| **seen != expected)
        .map(|seen| {
            format!(
                "the provider was given {} {} query {:?} body {:?}, expected the canonical {} {} query {:?} body {:?}",
                seen.method,
                seen.path,
                seen.query,
                String::from_utf8_lossy(&seen.body),
                expected.method,
                expected.path,
                expected.query,
                String::from_utf8_lossy(&expected.body),
            )
        })
        .collect()
}

/// An accepted request: success, and `subject` everywhere.
pub(crate) fn accepted(transport: Transport, answer: &Answer, subject: &str) -> Vec<String> {
    subjects(transport, answer)
        .into_iter()
        .filter_map(|got| match got {
            Ok(got) if got == subject => None,
            Ok(got) => Some(format!("the caller's subject was {got:?}, expected {subject:?}")),
            Err(answer) => Some(format!("the call was not accepted: {answer}")),
        })
        .collect()
}

/// A refused request: the status, code, message, members, `details` and
/// challenges of `expected`. `guard_challenge` is MCP's: its own
/// `Bearer ...` challenge comes first, then the provider's.
pub(crate) fn refused(transport: Transport, answer: &Answer, expected: &Expected) -> Vec<String> {
    let mut findings = Vec::new();
    if answer.status != expected.status {
        findings.push(format!(
            "status {}, expected {} (body {})",
            answer.status, expected.status, answer.body
        ));
        return findings;
    }
    let code = match transport {
        Transport::RpcUnary | Transport::RpcBatch => expected.rpc_code(),
        _ => expected.rest_code(),
    };
    let body = &answer.body;
    if body["code"] != Value::from(code) {
        findings.push(format!("code {}, expected {code:?}", body["code"]));
    }
    let mcp_message = transport == Transport::Mcp && expected.plain;
    if let Some(message) = &expected.message
        && !mcp_message
        && body["message"] != Value::from(message.as_str())
    {
        findings.push(format!("message {}, expected {message:?}", body["message"]));
    }
    let details = body.get("details").cloned().unwrap_or(Value::Null);
    if details != expected.details.clone().unwrap_or(Value::Null) {
        findings.push(format!("details {details}, expected {:?}", expected.details));
    }
    let members: Vec<(&String, &Value)> = body
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(name, _)| !["code", "message", "details"].contains(&name.as_str()))
        .collect();
    let wanted: Vec<(&String, &Value)> = expected.members.iter().collect();
    if members != wanted {
        findings.push(format!(
            "envelope members {members:?}, expected {wanted:?}"
        ));
    }
    let challenges = match transport {
        Transport::Mcp => match answer.challenges.split_first() {
            Some((guard, rest)) if guard.starts_with("Bearer ") => rest.to_vec(),
            _ => {
                findings.push(format!(
                    "MCP's own Bearer challenge must come first, got {:?}",
                    answer.challenges
                ));
                return findings;
            }
        },
        _ => answer.challenges.clone(),
    };
    if challenges != expected.challenges {
        findings.push(format!(
            "WWW-Authenticate {challenges:?}, expected {:?}",
            expected.challenges
        ));
    }
    findings
}

/// MCP under `ask_provider`, for a request whose credential the provider
/// says is absent: the bare challenge, before the provider runs.
pub(crate) fn absent_on_mcp(answer: &Answer) -> Vec<String> {
    let bare = answer.status == StatusCode::UNAUTHORIZED
        && answer.challenges.len() == 1
        && answer.challenges[0].starts_with("Bearer resource_metadata=");
    if bare {
        Vec::new()
    } else {
        vec![format!(
            "a credential the provider calls absent must get MCP's bare 401 challenge, got {} {:?}",
            answer.status, answer.challenges
        )]
    }
}

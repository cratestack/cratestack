//! The MCP half of idempotency (ADR 0002 Q6): what a key looks like on this
//! transport, whose namespace it lives in, and how a result is recorded and
//! replayed. The decision itself is L3's (`OpExecutor::admit`), unchanged.
//!
//! **The key** is `_meta["dev.cratestack/idempotencyKey"]`, held to the same
//! rules `cratestack-axum` applies to the `Idempotency-Key` header
//! (`idempotency/parse.rs`): a string, trimmed, non-empty, at most 255
//! characters, printable ASCII. Absent means no reservation — the same as
//! REST without the header.
//!
//! **The namespace** is `mcp:<principal actor id>`. HTTP derives it from a
//! hash of `Authorization` or the verified peer address, neither of which
//! exists on stdio; the caller here *is* the context the application
//! supplied, so its actor id is the identity to scope by. With no actor id
//! the call is refused rather than put in a shared `"anonymous"` namespace,
//! for cratestack#416's reason: two callers sharing a namespace can replay
//! each other's results. The `mcp:` prefix keeps these rows apart from
//! HTTP's in a store both transports share.
//!
//! **The record** is the serialized `CallToolResult`, with status 200 for a
//! success and the error's HTTP status for an `isError` result. As on HTTP,
//! an error outcome is recorded too: the IETF contract freezes the outcome,
//! whatever it was (`IdempotencyStore::complete`'s docs).

use cratestack_core::CratestackError;
use cratestack_core::idempotency_record::IdempotencyRecord;
use rmcp::model::{CallToolResult, RequestMetaObject};
use serde_json::Value;

use crate::{IDEMPOTENCY_KEY_META, IDEMPOTENCY_REPLAYED_META};

const MAX_KEY_LEN: usize = 255;

/// The key a call carries, if any. `context_meta` is where `rmcp` puts a
/// decoded request's `_meta`; `params_meta` covers a request built in
/// process. A malformed key is refused, never treated as absent: silently
/// dropping it would run a retry twice.
pub(crate) fn idempotency_key(
    context_meta: &RequestMetaObject,
    params_meta: Option<&RequestMetaObject>,
) -> Result<Option<String>, CratestackError> {
    let raw = context_meta
        .get(IDEMPOTENCY_KEY_META)
        .or_else(|| params_meta.and_then(|meta| meta.get(IDEMPOTENCY_KEY_META)));
    let Some(raw) = raw else {
        return Ok(None);
    };
    let Value::String(raw) = raw else {
        return Err(bad_key("must be a string"));
    };
    let key = raw.trim();
    if key.is_empty() {
        return Err(bad_key("must not be empty"));
    }
    if key.len() > MAX_KEY_LEN {
        return Err(bad_key("must be at most 255 characters"));
    }
    if !key
        .bytes()
        .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        return Err(bad_key("must be printable ASCII"));
    }
    Ok(Some(key.to_owned()))
}

fn bad_key(rule: &str) -> CratestackError {
    CratestackError::BadRequest(format!("_meta[\"{IDEMPOTENCY_KEY_META}\"] {rule}"))
}

/// `mcp:<actor id>`, or a refusal when the context has none.
pub(crate) fn namespace(actor_id: Option<&str>) -> Result<String, CratestackError> {
    match actor_id {
        Some(id) if !id.is_empty() => Ok(format!("mcp:{id}")),
        _ => Err(CratestackError::PreconditionFailed(
            "this server's context has no principal id, so a keyed or rate-limited call has no \
             namespace to be scoped to; build the context with an `id` claim"
                .to_owned(),
        )),
    }
}

/// What `OpExecutor::complete` stores for a finished call.
pub(crate) fn record(result: &CallToolResult, status: u16) -> (u16, Vec<u8>) {
    // A `CallToolResult` is plain data; if it ever failed to serialize, an
    // empty body makes the replay below refuse loudly instead of replaying
    // something else.
    (status, serde_json::to_vec(result).unwrap_or_default())
}

/// Rebuild a recorded result, marked as a replay.
pub(crate) fn replay(record: &IdempotencyRecord) -> Result<CallToolResult, CratestackError> {
    let mut result: CallToolResult =
        serde_json::from_slice(&record.response_body).map_err(|error| {
            CratestackError::Internal(format!(
                "mcp: a recorded idempotent result does not decode: {error}"
            ))
        })?;
    let mut meta = result.meta.take().unwrap_or_default();
    meta.insert(IDEMPOTENCY_REPLAYED_META.to_owned(), Value::Bool(true));
    result.meta = Some(meta);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use rmcp::model::RequestMetaObject;
    use serde_json::{Value, json};

    use super::{idempotency_key, namespace};

    fn meta(value: Value) -> RequestMetaObject {
        let mut meta = RequestMetaObject::new();
        meta.insert(crate::IDEMPOTENCY_KEY_META.to_owned(), value);
        meta
    }

    #[test]
    fn a_key_follows_the_idempotency_key_header_rules() {
        let empty = RequestMetaObject::new();
        assert_eq!(idempotency_key(&empty, None).unwrap(), None);
        assert_eq!(
            idempotency_key(&meta(json!("  k-1 ")), None).unwrap(),
            Some("k-1".to_owned())
        );
        assert_eq!(
            idempotency_key(&empty, Some(&meta(json!("k-2")))).unwrap(),
            Some("k-2".to_owned())
        );
        for bad in [json!(7), json!("   "), json!("x".repeat(256)), json!("é")] {
            let error = idempotency_key(&meta(bad.clone()), None).unwrap_err();
            assert_eq!(error.code(), "BAD_REQUEST", "{bad} must be refused");
        }
    }

    #[test]
    fn no_actor_id_means_no_namespace() {
        assert_eq!(namespace(Some("u-1")).unwrap(), "mcp:u-1");
        assert_eq!(namespace(None).unwrap_err().code(), "PRECONDITION_FAILED");
        assert_eq!(
            namespace(Some("")).unwrap_err().code(),
            "PRECONDITION_FAILED"
        );
    }
}

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
//! **The namespace** is `mcp:<sha256 of the principal actor id>` for a user
//! and `mcp-system:<sha256 of the actor id>` for a `SystemContext`, in
//! lowercase hex. REST derives its own from a hash of `Authorization`, a
//! verified principal or the peer address; the caller here *is* a context —
//! the one the application supplied on stdio, or the one its `AuthProvider`
//! built on Streamable HTTP — so its actor id is the identity to scope by.
//! With no actor id the call is refused rather than put in a shared
//! `"anonymous"` namespace, for cratestack#416's reason: two callers sharing
//! a namespace can replay each other's results.
//!
//! **Hashed, and never REST's** (maintainer decision on cratestack#1033,
//! from #1071's questions). The id is hashed for REST's reason
//! (`cratestack-axum`'s `VerifiedPrincipal`): an identifier never lands in a
//! store key verbatim. The same string is the rate-limit bucket
//! (`src/admission.rs`), and there the `mcp` prefixes are the point: a
//! caller's MCP calls and its REST calls draw on **separate budgets**, one
//! per transport, because ADR 0002's requirement 13 asks for the same L3
//! *admission*, not the same bucket. REST's keys start `princ:`, `auth:`,
//! `peer:` or `ip:`, so no MCP key can equal one; the
//! `an_mcp_namespace_is_never_a_rest_bucket` test pins that.
//!
//! **Why system callers get their own prefix** (maintainer decision on
//! cratestack#1039): `SystemContext::for_service("svc")`'s id is the string
//! `system:svc`, and on Streamable HTTP a token's `id` claim is whatever the
//! token says. Under one `mcp:` prefix a token claiming `id = "system:svc"`
//! would share that service's namespace and could replay its results. A
//! user's namespace always starts `mcp:` and a system one `mcp-system:`, so
//! no user id can produce a system namespace. `is_system()` is the only
//! input, and it cannot be forged from a request (`cratestack-core`'s
//! `context/system.rs`). The prefix stays outside the hash (below), so
//! hashing the id keeps this split.
//!
//! The id is read by [`principal_id`], not `principal_actor_id`, because
//! the latter answers only a *string* `id`: an `auth User { id Int }`
//! schema — the shape ADR 0002's own examples use — would otherwise be
//! refused on every keyed or rate-limited call, told to add the `id` claim
//! it already has. An `Int` id renders as its decimal digits, so `7` and
//! `"7"` share a namespace; one `auth` block declares one type for `id`,
//! so a single service never has both.
//!
//! **The record** is the serialized `CallToolResult`, with status 200 for a
//! success and the error's HTTP status for an `isError` result. As on HTTP,
//! an error outcome is recorded too: the IETF contract freezes the outcome,
//! whatever it was (`IdempotencyStore::complete`'s docs).

use std::borrow::Cow;

use cratestack_core::idempotency_record::IdempotencyRecord;
use cratestack_core::{CratestackContext, CratestackError, Value as ClaimValue};
use rmcp::model::{CallToolResult, RequestMetaObject};
use serde_json::Value;
use sha2::{Digest, Sha256};

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

/// The caller's `id` claim as text: the value `principal_actor_id` finds,
/// in its lookup order (actor facet, principal claims, auth fields; the
/// first *present* one wins), but answered for an `Int` as well as a
/// `String`. The `a_string_id_agrees_with_principal_actor_id` test keeps
/// the two from drifting apart.
pub(crate) fn principal_id(ctx: &CratestackContext) -> Option<Cow<'_, str>> {
    let principal = ctx.principal.as_ref();
    let id = principal
        .and_then(|principal| principal.actor.as_ref())
        .and_then(|facet| facet.fields.get("id"))
        .or_else(|| principal.and_then(|principal| principal.claims.get("id")))
        .or_else(|| ctx.auth.as_ref().and_then(|auth| auth.fields.get("id")))?;
    match id {
        ClaimValue::String(id) => Some(Cow::Borrowed(id.as_str())),
        ClaimValue::Int(id) => Some(Cow::Owned(id.to_string())),
        _ => None,
    }
}

/// `mcp:<sha256 hex of the actor id>` for a user, `mcp-system:<…>` for a
/// system caller, or a refusal when the context has no actor id.
pub(crate) fn namespace(ctx: &CratestackContext) -> Result<String, CratestackError> {
    let prefix = if ctx.is_system() { "mcp-system" } else { "mcp" };
    match principal_id(ctx) {
        Some(id) if !id.is_empty() => Ok(format!("{prefix}:{}", sha256_hex(id.as_bytes()))),
        _ => Err(CratestackError::PreconditionFailed(
            "this server's context has no principal id, so a keyed or rate-limited call has no \
             namespace to be scoped to; build the context with an `id` claim"
                .to_owned(),
        )),
    }
}

/// The byte-wise `{:02x}` fold `cratestack-axum`'s bucket keys use: sha2
/// 0.11's digest implements no `LowerHex`.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

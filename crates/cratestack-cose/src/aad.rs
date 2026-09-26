//! External AAD: the request context a message is bound to, encoded, never
//! sent (ADR 0006 §4, as amended while scoping P0 and by the maintainer's
//! decisions on cratestack#1005: `audience` on 2026-09-24, `request_kind`
//! on 2026-09-25).
//!
//! ```cddl
//! external_aad = bstr .cbor [
//!   1,                                  ; binding version
//!   audience: tstr,                     ; the recipient's configured id, non-empty
//!   method: tstr,
//!   route: tstr,                        ; RPC op_id; REST route template
//!   path_params: [* tstr],              ; REST: matched values in template order; RPC: []
//!   query: tstr / null,
//!   schema_sha: bstr .size 32,
//!   payload_type: tstr,
//!   ? request_kind: uint,               ; responses: 0 unsigned request, 1 signed
//!   ? request_digest: bstr .size 32,    ; responses, see request_digest*
//!   ? status: uint,                     ; responses
//! ]
//! ```
//!
//! Neither `audience` nor `request_kind` bumped the binding version:
//! nothing has been released with version 1 yet, so there is no older
//! layout to tell them apart from. `audience` sits right after the version
//! so that every other field keeps its relative position.
//!
//! Three rules the CDDL leaves open are pinned here, because the client and
//! the server each rebuild this array from their own context and any
//! disagreement is a `401` on every request:
//!
//! - **`query` is `null` when there is no query *or* it is empty.** A
//!   router that sees `/x?` and a client that built `/x` must agree, and
//!   the `?` alone carries nothing to bind.
//! - **The three response elements travel together**: 11 elements for a
//!   response, 8 for a request. `Binding` makes anything else
//!   unrepresentable (its response half is one `Option`).
//! - **An empty `audience` is refused** with a `500`, not encoded: it binds
//!   no recipient, so it would silently give up the cross-service and
//!   reflection protection the element exists for.
//!
//! Nothing is normalised beyond that: `method`, `route` and the path
//! parameter values are bound exactly as given. The array encoding keeps
//! the fields apart, so `["a", "b"]` and `["ab"]` bind differently.

use cratestack_core::{Binding, CratestackError, RequestDigest, RequestKind};
use sha2::{Digest, Sha256};

use crate::cbor::write::{self, MAJOR_ARRAY, MAJOR_UINT, NULL};
use crate::error::misuse;

/// The binding version, the first array element. Q5's escape hatch: a
/// future binding scheme gets a new number instead of a new wire format.
///
/// Version 1 is frozen at the first release in which generated routers and
/// clients use the envelope (cratestack#1006 / #1007), not at the first
/// release of this crate, which has no wire peers. After that release, any
/// change to the elements or to how one is derived bumps this number, and
/// verifiers reject versions they do not know (ADR 0006 §4).
pub const BINDING_VERSION: u64 = 1;

/// Encode the external AAD for `bind`: the bytes that go into the
/// `Sig_structure` / `MAC_structure` as `external_aad`. Fails with
/// `CratestackError::Internal` (misuse) for an empty `audience`.
///
/// Public so other language bindings and the shared vectors can check
/// their own encoding against this one.
pub fn external_aad(bind: &Binding<'_>) -> Result<Vec<u8>, CratestackError> {
    if bind.audience.is_empty() {
        return Err(misuse(
            "an empty audience binds no recipient; configure the service's logical id",
        ));
    }
    let query = bind.query.as_deref().filter(|query| !query.is_empty());
    let mut out =
        Vec::with_capacity(96 + bind.audience.len() + bind.route.len() + bind.method.len());
    let elements = if bind.response.is_some() { 11 } else { 8 };
    write::head(&mut out, MAJOR_ARRAY, elements);
    write::head(&mut out, MAJOR_UINT, BINDING_VERSION);
    write::tstr(&mut out, &bind.audience);
    write::tstr(&mut out, &bind.method);
    write::tstr(&mut out, &bind.route);
    write::head(
        &mut out,
        MAJOR_ARRAY,
        write::len_arg(bind.path_params.len()),
    );
    for value in bind.path_params.iter() {
        write::tstr(&mut out, value);
    }
    match query {
        Some(query) => write::tstr(&mut out, query),
        None => out.push(NULL),
    }
    write::bstr(&mut out, &bind.schema_sha);
    write::tstr(&mut out, &bind.payload_media_type);
    if let Some(response) = &bind.response {
        write::head(
            &mut out,
            MAJOR_UINT,
            u64::from(response.request.kind.code()),
        );
        write::bstr(&mut out, &response.request.digest);
        write::head(&mut out, MAJOR_UINT, u64::from(response.status));
    }
    Ok(out)
}

/// The request digest a response binding carries when the request was
/// **signed**: SHA-256 over the request body exactly as it travelled, the
/// whole COSE message (tag, headers, signature and all), marked
/// [`RequestKind::Signed`]. The caller passes the received body, never a
/// re-encoding of it.
///
/// For an unsigned request, use [`request_digest_unsigned`](crate::request_digest_unsigned),
/// which also binds the client's `Cratestack-Nonce`.
pub fn request_digest(signed_request_body: &[u8]) -> RequestDigest {
    RequestDigest {
        kind: RequestKind::Signed,
        digest: Sha256::digest(signed_request_body).into(),
    }
}

//! External AAD: the request context a message is bound to, encoded, never
//! sent (ADR 0006 §4, as amended while scoping P0, by the maintainer's
//! decisions on cratestack#1005: `audience` on 2026-09-24, `request_kind`
//! on 2026-09-25, and by decision S1 after the cratestack#1006 security
//! review: `bound_headers` on 2026-09-26).
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
//!   bound_headers: [                    ; request headers with semantics,
//!     idempotency_key: tstr / null,     ;   exactly as sent; null if absent
//!     if_match: tstr / null,
//!   ],
//!   ? request_kind: uint,               ; responses: 0 unsigned request, 1 signed
//!   ? request_digest: bstr .size 32,    ; responses, see request_digest*
//!   ? status: uint,                     ; responses
//! ]
//! ```
//!
//! Neither `audience`, `request_kind` nor `bound_headers` bumped the
//! binding version: nothing has been released with version 1 yet, so there
//! is no older layout to tell them apart from. `audience` sits right after
//! the version so that every other field keeps its relative position;
//! `bound_headers` is one fixed-length array, so a request binding is 9
//! elements and a response binding 12.
//!
//! `bound_headers` exists because an on-path party could otherwise strip
//! or swap those headers on a signed request without breaking its
//! signature: dropping `Idempotency-Key` from a re-sealed retry runs the
//! operation twice, and dropping `If-Match` turns a conditional update into
//! a blind one. Their values are bound as the header carried them, with no
//! normalisation (see `cratestack_core::BoundHeaders`).
//!
//! Three rules the CDDL leaves open are pinned here, because the client and
//! the server each rebuild this array from their own context and any
//! disagreement is a `401` on every request:
//!
//! - **`query` is `null` when there is no query *or* it is empty.** A
//!   router that sees `/x?` and a client that built `/x` must agree, and
//!   the `?` alone carries nothing to bind.
//! - **The three response elements travel together**: 12 elements for a
//!   response, 9 for a request. `Binding` makes anything else
//!   unrepresentable (its response half is one `Option`).
//! - **An empty `audience` is refused** with a `500`, not encoded: it binds
//!   no recipient, so it would silently give up the cross-service and
//!   reflection protection the element exists for.
//!
//! Nothing is normalised beyond that: `method`, `route`, the path
//! parameter values and the bound header values are bound exactly as given
//! (an empty header value is bound as `""`, not as `null`). The array encoding keeps
//! the fields apart, so `["a", "b"]` and `["ab"]` bind differently.

use cratestack_core::{Binding, CratestackError};

use crate::cbor::write::{self, MAJOR_ARRAY, MAJOR_UINT, NULL};
use crate::error::misuse;

/// The binding version, the first array element. Q5's escape hatch: a
/// future binding scheme gets a new number instead of a new wire format.
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
    let headers = &bind.bound_headers;
    let bound = [&headers.idempotency_key, &headers.if_match];
    // 3 more than before `bound_headers`: its array head and two `null`s.
    let mut out = Vec::with_capacity(
        99 + bind.audience.len()
            + bind.route.len()
            + bind.method.len()
            + bound
                .iter()
                .map(|value| value.as_deref().map_or(0, str::len))
                .sum::<usize>(),
    );
    let elements = if bind.response.is_some() { 12 } else { 9 };
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
    write::head(&mut out, MAJOR_ARRAY, 2);
    for value in bound {
        match value {
            Some(value) => write::tstr(&mut out, value),
            None => out.push(NULL),
        }
    }
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

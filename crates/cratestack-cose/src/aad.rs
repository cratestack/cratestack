//! External AAD: the request context a message is bound to, encoded, never
//! sent (ADR 0006 §4, as amended while scoping P0).
//!
//! ```cddl
//! external_aad = bstr .cbor [
//!   1,                                  ; binding version
//!   method: tstr,
//!   route: tstr,                        ; RPC op_id; REST route template
//!   path_params: [* tstr],              ; REST: matched values in template order; RPC: []
//!   query: tstr / null,
//!   schema_sha: bstr .size 32,
//!   payload_type: tstr,
//!   ? request_digest: bstr .size 32,    ; responses
//!   ? status: uint,                     ; responses
//! ]
//! ```
//!
//! Two rules the CDDL leaves open are pinned here, because the client and
//! the server each rebuild this array from their own context and any
//! disagreement is a `401` on every request:
//!
//! - **`query` is `null` when there is no query *or* it is empty.** A
//!   router that sees `/x?` and a client that built `/x` must agree, and
//!   the `?` alone carries nothing to bind.
//! - **`request_digest` and `status` travel together.** Both present is a
//!   response binding (9 elements), both absent a request binding (7). One
//!   without the other is a local bug and is refused with a `500` rather
//!   than encoded as some third shape.
//!
//! Nothing is normalised beyond that: `method`, `route` and the path
//! parameter values are bound exactly as given. The array encoding keeps
//! the fields apart, so `["a", "b"]` and `["ab"]` bind differently.

use cratestack_core::{Binding, CratestackError};
use sha2::{Digest, Sha256};

use crate::cbor::write::{self, MAJOR_ARRAY, MAJOR_UINT, NULL};
use crate::error::misuse;

/// The binding version, the first array element. Q5's escape hatch: a
/// future binding scheme gets a new number instead of a new wire format.
pub const BINDING_VERSION: u64 = 1;

/// Whether a [`Binding`] describes a request or a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Request,
    Response { digest: [u8; 32], status: u16 },
}

/// Classify `bind`, refusing the half-response shape.
pub(crate) fn direction(bind: &Binding<'_>) -> Result<Direction, CratestackError> {
    match (bind.request_digest, bind.status) {
        (None, None) => Ok(Direction::Request),
        (Some(digest), Some(status)) => Ok(Direction::Response { digest, status }),
        _ => Err(misuse(
            "a binding sets request_digest and status together, or neither",
        )),
    }
}

/// Encode the external AAD for `bind`: the bytes that go into the
/// `Sig_structure` / `MAC_structure` as `external_aad`.
///
/// Public so other language bindings and the shared vectors can check
/// their own encoding against this one.
pub fn external_aad(bind: &Binding<'_>) -> Result<Vec<u8>, CratestackError> {
    let direction = direction(bind)?;
    let query = bind.query.as_deref().filter(|query| !query.is_empty());
    let mut out = Vec::with_capacity(96 + bind.route.len() + bind.method.len());
    let elements = match direction {
        Direction::Request => 7,
        Direction::Response { .. } => 9,
    };
    write::head(&mut out, MAJOR_ARRAY, elements);
    write::head(&mut out, MAJOR_UINT, BINDING_VERSION);
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
    if let Direction::Response { digest, status } = direction {
        write::bstr(&mut out, &digest);
        write::head(&mut out, MAJOR_UINT, u64::from(status));
    }
    Ok(out)
}

/// The `request_digest` a response binding carries: SHA-256 over the
/// request body **exactly as it travelled**. For a signed request that is
/// the whole COSE message (tag, headers, signature and all); for an
/// unsigned one it is the plain payload. The same function covers both
/// because both are "the bytes of the request body"; the caller passes the
/// body, never a re-encoding of it.
pub fn request_digest(request_body: &[u8]) -> [u8; 32] {
    Sha256::digest(request_body).into()
}

//! [`Binding`]: the request context an envelope binds a body to (ADR 0006
//! §1, §4).

use std::borrow::Cow;

use super::path_params::PathParams;

/// Everything that goes into a sealed body's COSE `external_aad` (ADR 0006
/// §4). It is **never sent**: the router and the client each rebuild it
/// from context they already hold, so the binding costs 0 bytes on the
/// wire. A body sealed for one `(audience, method, route, path_params,
/// query, schema)` fails to open under any other, which defeats
/// cross-service and cross-endpoint replay; a response is bound to its request's digest and its status,
/// which defeats response swapping; a signed REST response is bound to the
/// resource it answers, not only to the route's shape; and a client built
/// against another `.cstack` fails closed on `schema_sha`.
///
/// This type carries the **inputs** only. Encoding them into the AAD's CBOR
/// array, including the leading binding-version field (currently `1`, the
/// escape hatch ADR 0006 Q5 reserves), belongs to `cratestack-cose`
/// (cratestack#1005). Core has no CBOR dependency (the CBOR backend lives
/// in `cratestack-codec-cbor`), and every crate depends on core, so it
/// should not gain one here. Keeping the byte layout in one crate also
/// keeps it to one encoder that every client shares (§11).
///
/// A request binding leaves `request_digest` and `status` as `None`. A
/// response binding sets both. The AAD encoder decides what a mismatch
/// means; this type does not validate the combination.
///
/// **Deviation from the ADR 0006 §1 sketch**, which uses `&'a str`,
/// `&'a [&'a str]` and `&'a [u8; 32]`: the string fields are [`Cow`],
/// `path_params` is the `Cow`-like [`PathParams`], and `schema_sha` is held
/// by value. The unary path still borrows everything and allocates nothing.
/// The stream path can also produce the `Binding<'static>` that
/// [`CratestackEnvelope::stream_sealer`](super::CratestackEnvelope::stream_sealer)
/// takes, through [`Binding::into_owned`]. A sealer outlives the request
/// borrow it was created from, so it cannot hold `&'a` fields. Copying 32
/// bytes is cheaper than a second lifetime parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding<'a> {
    /// The logical identifier of the service the message is addressed to,
    /// e.g. `"payments"`: a **configured** name that the sender and the
    /// receiver agree on, never the `Host` header (a proxy rewrites it, and
    /// the sender does not control it). A server builds its bindings with
    /// its own inbound audience; a client, with the audience of the service
    /// it calls. The AAD encodes it right after the binding version.
    ///
    /// Two attacks need it, both of which a shared schema and route would
    /// otherwise let through (maintainer decision on cratestack#1005,
    /// 2026-09-24): a request sealed for service A replayed against service
    /// B, and a COSE_Mac0 request that service A sent out, reflected back to
    /// A itself (the MAC key is symmetric, so A cannot tell its own message
    /// from its peer's by the key). Both fail once A's inbound audience
    /// differs from the audience it seals outbound requests for.
    ///
    /// Added after cratestack#1004 merged, which made this a breaking
    /// change to a public-field struct. That is acceptable pre-1.0 with no
    /// downstream user yet.
    pub audience: Cow<'a, str>,
    /// HTTP method, e.g. `"POST"`.
    pub method: Cow<'a, str>,
    /// The RPC `op_id` (e.g. `model.Payment.create`), or the REST route
    /// template (e.g. `/accounts/{id}`). The template alone names the
    /// route's shape, not the resource; the matched values are bound
    /// separately in [`path_params`](Self::path_params). **Never the raw URL
    /// path**: gateways rewrite prefixes, and a binding that changes under
    /// `Router::nest` fails every legitimate request (the same fact broke
    /// descriptor lookup in cratestack#877).
    pub route: Cow<'a, str>,
    /// REST: the matched path parameter values, in the order the route
    /// template declares them, as the router decoded them. RPC: empty
    /// ([`PathParams::EMPTY`]), because the `op_id` and the signed body
    /// (for a response, the request digest) already bind it. Without these,
    /// a signed response to `GET /accounts/1` would verify as the answer to
    /// `GET /accounts/2` (ADR 0006 §4, amended 2026-09-24). The AAD encodes
    /// them right after `route`.
    pub path_params: PathParams<'a>,
    /// The request's canonicalised query string (the same canonical form
    /// `cratestack-auth`'s `canonical_query` produces), or `None` when the
    /// request has none.
    pub query: Option<Cow<'a, str>>,
    /// The schema's SHA-256 as raw bytes. It is the same digest that
    /// generated code bakes in as the hex string `SCHEMA_SHA256` and sends
    /// in `x-cratestack-schema-sha`. That digest hashes the raw `.cstack`
    /// text today, so a comment-only edit changes it; what the AAD should
    /// bind is settled before `Required` mode ships (ADR 0006, P0 scoping
    /// decisions).
    pub schema_sha: [u8; 32],
    /// Media type of the payload *inside* the envelope, e.g.
    /// `"application/cbor"`. It is bound here instead of being sent in a
    /// COSE header (label 3 would cost bytes on every message; ADR 0006 §2),
    /// so a verifier cannot be tricked into decoding the payload as
    /// something else.
    pub payload_media_type: Cow<'a, str>,
    /// Responses only: SHA-256 over the request's COSE bytes when the
    /// request was signed. When it was not (a bodiless `GET`, say), SHA-256
    /// over the 16-byte `Cratestack-Nonce` the client sent followed by the
    /// payload, so that a signed response answers exactly one request and a
    /// cached one cannot be replayed for the next `GET` of the same URL
    /// (maintainer decision on cratestack#1005). `cratestack-cose` provides
    /// both digests.
    pub request_digest: Option<[u8; 32]>,
    /// Responses only: the HTTP status code.
    pub status: Option<u16>,
}

impl Binding<'_> {
    /// Detach from the borrowed request data, for a stream sealer or opener
    /// that outlives the request. This allocates for every field that is
    /// still borrowed. A caller whose fields are already `'static` (an
    /// `op_id` from generated code, a codec's `CONTENT_TYPE`) can build a
    /// `Binding<'static>` directly with `Cow::Borrowed` and skip the copy.
    pub fn into_owned(self) -> Binding<'static> {
        Binding {
            audience: Cow::Owned(self.audience.into_owned()),
            method: Cow::Owned(self.method.into_owned()),
            route: Cow::Owned(self.route.into_owned()),
            path_params: self.path_params.into_owned(),
            query: self.query.map(|query| Cow::Owned(query.into_owned())),
            schema_sha: self.schema_sha,
            payload_media_type: Cow::Owned(self.payload_media_type.into_owned()),
            request_digest: self.request_digest,
            status: self.status,
        }
    }
}

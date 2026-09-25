//! The server envelope layer (ADR 0006 §§2, 10, 12; cratestack#1006): opens
//! signed requests and seals responses for the generated REST and RPC
//! routers, before rate limiting and idempotency run.
//!
//! Behind this crate's `cose` feature (maintainer decision D7), which the
//! `cratestack-pg` and `cratestack-api` facades forward. Without it no
//! COSE, P-256 or Ed25519 code is in their graphs (CI's
//! `facade-disjointness` job asserts it).
//!
//! # Placement (D12)
//!
//! The layer must be the **last** `.layer(..)` on the generated router, and
//! applied before the router is `merge`d or `nest`ed:
//!
//! ```text
//! let router = cratestack_schema::axum::router(db, procedures, codec, auth, limit)
//!     .layer(IdempotencyLayer::new(store, ttl))
//!     .layer(RateLimitLayer::new(buckets, config))
//!     .layer(envelope_layer); // runs first
//! let app = axum::Router::new().nest("/api", router); // prefix "/api" on the layer
//! ```
//!
//! - Last, so it runs first: the rate limiter and the idempotency layer key
//!   on the `VerifiedPrincipal` it inserts (§12), and the idempotency layer
//!   hashes the opened payload, so a client that re-seals a retry (a new
//!   `cti` under the same `Idempotency-Key`) gets the stored response,
//!   sealed afresh for the new request.
//! - Through `Router::layer`, so `MatchedPath` and the path parameters are
//!   already in the request; a `ServiceBuilder` around the whole app loses
//!   them, and `nest_service` never sets `MatchedPath`.
//! - Verification, including the key resolver and nonce store lookups, now
//!   runs **before** rate limiting. Put an IP-level limiter outside this
//!   layer to bound what an unauthenticated flood can cost.
//!
//! # What the layer decides, per request
//!
//! 1. The [`BindingResolver`] names the generated op (route and path
//!    parameters), once. No op (an unmatched path, a route the schema did
//!    not generate): the request passes through untouched (D6), unless it
//!    carries a COSE body, which is refused (see invariants).
//! 2. The [`EnvelopePolicy`] picks [`EnvelopeMode::Required`], `Optional`
//!    or `Off` for that op. There is no default (D9).
//! 3. A request whose `Content-Type` is an envelope type is opened. Under
//!    `Required` a request that is not is refused. Under `Optional` it runs
//!    unsigned, and its response is sealed only if it carries a valid
//!    `Cratestack-Nonce` and the [`ResponseSealPolicy`] agrees (by default,
//!    its `Accept` names `application/cose`; D10).
//! 4. An opened request reaches the router as the plain CBOR request it
//!    wraps: body replaced by the payload, `Content-Type: application/cbor`
//!    (removed for an empty payload), and `Accept` set to
//!    `application/cbor`, so the handler produces the one representation
//!    the response binding names. Its extensions gain the
//!    `cratestack_core::VerifiedSigner` (recorded on the handler's context,
//!    D2, never an authentication) and the [`PrincipalMapper`]'s
//!    `VerifiedPrincipal`.
//! 5. The response is sealed with a binding built from the same values the
//!    request was opened against, plus the request digest and the status.
//!    Every response of a generated op is sealed, errors included (a
//!    handler's 404, the rate limiter's 429, the idempotency layer's 412 or
//!    422), except the layer's own refusals (D4, below).
//!
//! # Security invariants, enforced here and not by the plug-ins
//!
//! - A request whose `Content-Type` has the base type `application/cose`
//!   (any case, any parameters), or one [`ServerEnvelope::is_envelope_content_type`]
//!   claims, is opened and verified before anything else sees it, whatever
//!   the policy says. If the layer will not open it (policy `Off`, or no
//!   generated op to bind it to) it is refused with `415`, unsigned: never
//!   forwarded, so no inner layer, `AuthProvider` or codec ever treats
//!   unverified COSE bytes as a plain body.
//! - Under `Required`, no unsigned request reaches the inner service, and
//!   no response goes out plain: one the layer cannot seal (a stream, a
//!   body that is not CBOR) is replaced by a sealed error.
//! - A verification failure is always the same coarse `401`, whatever the
//!   envelope's error said, and it is **unsigned** (D4): a replayed request
//!   must not earn a signed "401" for a request that already executed. Any
//!   other envelope error (a key resolver or nonce store outage) is a
//!   `500` whose detail is only logged.
//! - The [`PrincipalMapper`] only ever sees a [`VerifiedRequest`], which
//!   only this layer can construct, after the envelope verified.
//! - The resolver and the policy are consulted once per request, and the
//!   response binding reuses the values the request was opened against,
//!   so no plug-in can make the two bindings disagree. The audience, the
//!   schema digest, the method, the canonical query, the payload media type
//!   and the request digest are the layer's own.
//!
//! # Known limits (P0)
//!
//! - Streamed responses (`@stream` over `application/cbor-seq`, SSE
//!   subscriptions) cannot be sealed until `chain` mode (ADR 0006 P1). Under
//!   `Required` the forced `Accept` makes a `@stream` op answer with one
//!   buffered, sealed array, and a subscription is refused with a sealed
//!   `406`; give those ops `Optional` through the policy to keep them.
//! - Only the body and the status are authenticated. Response headers
//!   (`ETag`, `Retry-After`, ...) are not.
//! - The schema digest hashes the raw `.cstack` text, so a comment-only
//!   edit changes it and breaks every signed client (cratestack#1065).
//!   `Required` is opt-in until that is settled.
//! - One envelope per layer: a router accepting Sign1 devices and Mac0
//!   services at once needs the composite of cratestack#1078.
//! - The response payload is copied once into the sealed message (D1; the
//!   zero-copy head/tail API is cratestack#1076).

mod cose_impl;
mod layer;
mod media;
mod mode;
mod principal;
mod refusal;
mod request;
mod resolver;
mod seal;
mod seal_policy;
mod server_envelope;
mod service;
mod signed;
mod unsigned;

#[cfg(test)]
mod tests;

pub use layer::{EnvelopeLayer, EnvelopeLayerBuilder};
pub use mode::{EnvelopeMode, EnvelopePolicy};
pub use principal::{PrincipalMapper, ThumbprintPrincipal, VerifiedRequest};
pub use resolver::{
    BindingResolver, ResolvedRoute, RestBindingResolver, RouteRequest, RpcBindingResolver,
};
pub use seal_policy::{AcceptNamesEnvelope, ResponseSealPolicy, UnsignedRequest};
pub use server_envelope::{OpenedRequest, ServerEnvelope};
pub use service::EnvelopeService;

/// The media type of every payload inside a sealed message: the AAD binds
/// it (ADR 0006 §4), and it is fixed to CBOR in P0.
pub const PAYLOAD_MEDIA_TYPE: &str = "application/cbor";

/// The default cap on a request body the layer buffers: the generated
/// routers' default body limit plus 16 KiB of envelope overhead (a header,
/// a signature, and room for a post-quantum one later, ADR 0006 Q5).
pub const DEFAULT_MAX_BODY_BYTES: usize = cratestack_core::DEFAULT_BODY_LIMIT_BYTES + 16 * 1024;

//! The server envelope layer (ADR 0006 §§2, 10, 12; cratestack#1006): opens
//! signed requests and seals responses for the generated REST and RPC
//! routers, before rate limiting and idempotency run.
//!
//! Feature `envelope` (the layer and its traits, no crypto crate), or `cose`
//! (adds `cratestack_cose::CoseEnvelope` as the [`ServerEnvelope`],
//! decision D7); the `cratestack-pg` and `cratestack-api` facades forward
//! both, and each schema gets a generated
//! `cratestack_schema::axum::envelope_layer(envelope, policy, audience)`
//! that picks its transport and schema digest.
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
//! let app = axum::Router::new().nest("/api", router); // mount_prefix("/api") on the layer
//! ```
//!
//! - Last, so it runs first: the rate limiter and the idempotency layer key
//!   on the `VerifiedPrincipal` it inserts (§12), and the idempotency layer
//!   hashes the opened payload, so a client that re-seals a retry (a new
//!   `cti` under the same `Idempotency-Key`) gets the stored response,
//!   sealed afresh for the new request.
//! - Through `Router::layer`, so `MatchedPath` and the path parameters are
//!   already in the request; a `ServiceBuilder` around the whole app loses
//!   them, and `nest_service` never sets `MatchedPath` for the outer mount.
//! - Verification, including the key resolver and nonce store lookups,
//!   runs **before** rate limiting, and under `Optional` so does signing
//!   every response to a signed or nonce-bound request. Put an IP-level
//!   limiter outside this layer to bound what an unauthenticated flood can
//!   cost.
//!
//! # What the layer decides, per request
//!
//! 1. A bodiless `OPTIONS` (a CORS preflight) passes through untouched.
//! 2. The [`BindingResolver`] names the generated op (route and path
//!    parameters), once. Not an op: see [`Resolution`] (D6; under a
//!    `Required` policy a matched route nobody resolved fails closed, S2).
//! 3. The [`EnvelopePolicy`] picks [`EnvelopeMode::Required`], `Optional`
//!    or `Off` for the op, seeing a [`PolicyRequest`] (no headers). There
//!    is no default (D9). A `/rpc/batch` call runs under the strictest mode
//!    of `batch` and every frame's op (B1).
//! 4. A request whose `Content-Type` is an envelope type is opened. Under
//!    `Required` a request that is not is refused. Under `Optional` it runs
//!    unsigned, and its response is sealed only if it carries a valid
//!    `Cratestack-Nonce` and the [`ResponseSealPolicy`] agrees (D10).
//! 5. An opened request reaches the router as the plain CBOR request it
//!    wraps, with `Accept: application/cbor`, and gains the
//!    `cratestack_core::VerifiedSigner` (recorded on the handler's context,
//!    D2, never an authentication) and the [`PrincipalMapper`]'s
//!    `VerifiedPrincipal` extensions.
//! 6. The response is sealed with a binding built from the same values the
//!    request was opened against, plus the request digest and the status.
//!    Every response to a signed request is sealed, under `Optional` too
//!    (S3), errors included, except the layer's own refusals (D4).
//!
//! # Security invariants, enforced here and not by the plug-ins
//!
//! - A request whose `Content-Type` has the base type `application/cose`
//!   (any case, any parameters), or one [`ServerEnvelope::is_envelope_content_type`]
//!   claims, is opened and verified before anything else sees it, whatever
//!   the policy says, or refused with the unsigned `415`: never forwarded.
//! - Under `Required`, no unsigned request reaches the inner service, and
//!   no response to a signed request goes out plain: one the layer cannot
//!   seal (a stream, a body that is not CBOR) is replaced by a sealed error.
//! - A verification failure is always the same coarse, **unsigned** `401`
//!   (D4): a replayed request must not earn a signed "401" for a request
//!   that already executed. Any other envelope error is a `500` whose
//!   detail is only logged.
//! - The [`PrincipalMapper`] only ever sees a [`VerifiedRequest`], which
//!   only this layer can construct, after the envelope verified.
//! - The resolver and the policy are consulted once per request, and the
//!   response binding reuses the values the request was opened against,
//!   so no plug-in can make the two bindings disagree. The audience, the
//!   schema digest, the method, the canonical query, the bound headers, the
//!   payload media type and the request digest are the layer's own.
//!
//! # What is bound, and what is not
//!
//! The AAD binds the audience, method, route, path parameters, canonical
//! query (distinct keys in any order bind alike; one key's repeated values
//! keep their order), schema digest, payload media type, and the
//! `Idempotency-Key` and `If-Match` headers exactly as sent (S1; a request
//! sending either twice is refused with a `400`). For a response: the
//! request digest and the status. **Response headers** (`ETag`,
//! `Retry-After`, ...) **are not authenticated.** An `Optional` response
//! sealed for an *unsigned* request binds its nonce and payload, not the
//! caller: it proves this server answered, not who asked.
//!
//! # Known limits (P0)
//!
//! - Streamed responses (`@stream` over `application/cbor-seq`, SSE
//!   subscriptions) cannot be sealed until `chain` mode (ADR 0006 P1). A
//!   signed request gets `Accept: application/cbor`, so a `@stream` op
//!   answers with one buffered, sealed array; a signed subscription is
//!   refused with a sealed `406` before its handler runs. Only an unsigned
//!   request under `Optional` can stream (plain).
//! - A response is re-buffered to be sealed, up to
//!   `cratestack_core::MAX_RESPONSE_REBUFFER_BYTES`; a longer one becomes a
//!   sealed `500`. A request body is buffered up to the layer's
//!   `max_body_bytes` (`413` beyond it).
//! - A fallback handler (`Router::fallback`) sets no `MatchedPath`, so the
//!   layer treats its traffic as unmatched and lets plain requests through.
//! - The schema digest hashes the raw `.cstack` text, so a comment-only
//!   edit changes it and breaks every signed client (cratestack#1065).
//!   `Required` is opt-in until that is settled.
//! - One envelope per layer: a router accepting Sign1 devices and Mac0
//!   services at once needs the composite of cratestack#1078, which the
//!   per-request [`SealContext`] and [`Sealed`] media type make possible.
//! - The response payload is copied once into the sealed message (D1; the
//!   zero-copy head/tail API is cratestack#1076).

mod batch;
mod bound;
mod builder;
#[cfg(feature = "cose")]
mod cose_impl;
mod dispatch;
mod inputs;
mod layer;
mod media;
mod mode;
mod opened;
mod policy_request;
mod principal;
mod refusal;
mod request;
mod resolver;
mod resolver_rest;
mod resolver_rpc;
mod seal;
mod seal_policy;
mod server_envelope;
mod service;
mod signed;
mod unresolved;
mod unsigned;

#[cfg(test)]
mod tests;

/// For implementing [`ServerEnvelope`] and [`PrincipalMapper`] without a
/// direct `async-trait` dependency.
pub use async_trait::async_trait;
pub use builder::EnvelopeLayerBuilder;
pub use layer::EnvelopeLayer;
pub use mode::{EnvelopeMode, EnvelopePolicy};
pub use opened::{OpenedRequest, SealContext, Sealed};
pub use policy_request::PolicyRequest;
pub use principal::{PrincipalMapper, ThumbprintPrincipal, VerifiedRequest};
pub use resolver::{BindingResolver, Resolution, ResolvedRoute, RouteRequest};
pub use resolver_rest::RestBindingResolver;
pub use resolver_rpc::RpcBindingResolver;
pub use seal_policy::{AcceptNamesEnvelope, ResponseSealPolicy, UnsignedRequest};
pub use server_envelope::ServerEnvelope;
pub use service::EnvelopeService;

/// The media type of every payload inside a sealed message: the AAD binds
/// it (ADR 0006 §4), and it is fixed to CBOR in P0.
pub const PAYLOAD_MEDIA_TYPE: &str = "application/cbor";

/// The default cap on a request body the layer buffers: the generated
/// routers' default body limit plus 16 KiB of envelope overhead (a header,
/// a signature, and room for a post-quantum one later, ADR 0006 Q5).
pub const DEFAULT_MAX_BODY_BYTES: usize = cratestack_core::DEFAULT_BODY_LIMIT_BYTES + 16 * 1024;

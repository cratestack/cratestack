## Design proposal: trusted-proxy configuration for audit `client_ip` (#415)

> **Status: proposal, not a decision.** This document exists so the maintainer can make the judgement calls listed under "Decisions needed"; it is not an approved design. Nothing here is implemented.

**Status:** proposal, awaiting maintainer decision — no code written. Revised after
adversarial review against the actual code and doc history; see **Reviewer notes** at
the end for a diff against the original draft.

### Decisions needed

1. **Config surface shape.** A consumer-applied mechanism (`tower::Layer` or a plain
   `Extension`) wired outside the generated `router()` — mirroring `RateLimitLayer`/
   `IdempotencyLayer` — or a new parameter threaded through `router(db, registry, codec,
   auth_provider)` and `ModelRouterState`.
2. **Implementation mechanism**, once (1) picks "outside `router()`": a bespoke
   `TrustedProxyLayer` + `Service` + a second `Extension<ResolvedClientIp>` type, or a
   single `Extension<TrustedProxyConfig>` resolved inline where `enrich_context_from_headers`
   already has to change. The latter is less new surface for the same non-breaking property.
3. **Safe default when nothing is configured.** Record no `client_ip` (`None`) rather than
   either trusting headers (today's bug) or guessing at an unavailable peer address.
4. **Allowlist shape for v1.** Exact `IpAddr` match only, or CIDR support now. No directly
   usable CIDR crate exists in the workspace's own `Cargo.toml` today, though `ipnet` is
   already present transitively (pulled in by `hyper-util`) — so adding it directly promotes
   an existing dependency-tree member rather than introducing an entirely new one, which
   modestly weakens (but doesn't reverse) the case for deferring CIDR to v2.
5. **Hop-count algorithm, precisely.** `max_hops` must be defined as "walk `max_hops`
   entries in from the right end of the `Forwarded`/`X-Forwarded-For` chain (the end nearest
   the trusted proxy) and take that entry," not "take the first `max_hops`-th entry from the
   left." The left end of the header is exactly the part an untrusted client controls — an
   algorithm that walks from the left re-opens the same spoofing gap for any chain with more
   than one hop. This needs to be nailed down in the design doc itself, not left implicit in
   the implementation.
6. **Scope: does this cover the gRPC transport?** `transport grpc` schemas build a *second*,
   separately-served `axum::Router` via `into_router()`
   (`crates/cratestack-macros/src/include/server/grpc/service.rs`), not the `router()`
   this proposal's two options both modify. Its handlers funnel through the same 7
   generated dispatch functions (confirmed — `grpc/service.rs` calls
   `super::axum::#dispatch_ident(...)` directly, reusing `handlers_crud.rs`/`handlers_update.rs`/etc.
   verbatim), so the *code path* is shared, but the *router instance* is not: a consumer
   who applies the protection only to `router()`'s output leaves gRPC-transport requests
   exactly as exposed as before the fix. Either explicitly scope this out with a tracked
   follow-up, or the migration note must say "apply to both `router()` and `into_router()`."
7. **Breaking-adjacent surface pre-1.0.** `enrich_context_from_headers` — public, re-exported
   as `cratestack::enrich_context_from_headers`, documented in `cratestack-axum/README.md` —
   changes signature under *both* options, so "Option A is non-breaking" is only true of
   `router()`'s signature specifically, not of the feature's blast radius as a whole.
   Separately: this workspace's own recent history (commit `32f89de`, PR #453/#454, merged
   the same day this issue was triaged) shipped a breaking `async fn authorize` signature
   change to a public trait (`RequestAuthorizer` in `crates/cratestack-client-rust/src/auth.rs`)
   directly, with no non-breaking alternative sought. That's relevant context for how much
   weight "breaking: true" should carry pre-1.0 in this repo — it should inform, not decide,
   the choice below.
8. **Where the integration test actually lives.** `cratestack-axum` has no dependency on
   `cratestack-macros` (confirmed — its `Cargo.toml` depends on `cratestack-core`, `axum`,
   `tower`, etc., nothing macro- or schema-related) and so cannot itself invoke
   `include_server_schema!`. A real end-to-end macro-integration test has to live where that
   macro is actually callable with no Postgres dependency — `crates/cratestack-api/tests/`,
   mirroring the existing `crates/cratestack-api/tests/no_database_procedures.rs` pattern
   (`include_server_schema!(..., db = None)`, no `cratestack-sqlx` in the dependency tree at
   all). "`cargo test -p cratestack-axum`" for this specific test, as the original draft
   stated, is not achievable as scoped.

### Options

| | A' — `Extension<TrustedProxyConfig>` (consumer-applied, resolved inline) | B — threaded through `router()`/state |
|---|---|---|
| **Shape** | Consumer adds `.layer(Extension(TrustedProxyConfig::trusting(ips).max_hops(n)))` (or `::none()`, the default) plus `.into_make_service_with_connect_info::<SocketAddr>()`; resolution happens inline inside the (already-changing) `enrich_context_from_headers` | New param on generated `router()`, new field on `ModelRouterState` |
| **Breaking** | `router()` signature unchanged. `enrich_context_from_headers`'s signature still changes either way — see decision 7 | Yes — every consumer's bootstrap call site breaks, on top of the same `enrich_context_from_headers` change |
| **Matches precedent** | Consistent with `RateLimitLayer`/`IdempotencyLayer`'s "app wires deployment-tier config at startup via `.layer(...)`" pattern — real, confirmed in `crates/cratestack-axum/src/ratelimit/` and `.../idempotency/`. The stronger claim in the original draft — that `docs/design/idempotency-rate-limit-declarative-surface.md` forbids Option B outright — overstates the doc: its actual axis is `.cstack`-declarative-vs-imperative, and it names `db = Postgres` (a macro-invocation parameter, structurally what B proposes) as a legitimate example of config living outside `.cstack`. Don't cite it as a hard "non-goal" B violates; it doesn't say that | Not itself forbidden by the doc, but still costs every consumer a mechanical bootstrap edit for a feature most deployments won't tune away from the default |
| **Connect-info footgun** | A consumer can forget the layer/extension: fails safe, `client_ip: None` | Identical footgun, mislabeled as A-only in the original draft: B still needs `ConnectInfo<SocketAddr>` extraction, which still depends on `into_make_service_with_connect_info` being wired correctly. B's "forces a decision at compile time" only covers the config *value*, not the connect-info wiring, which is the part most likely to be forgotten in practice |
| **Plumbing depth** | One combined extractor added at the 7 real call sites (not 8 — see below) → inline resolution in `enrich_context_from_headers` | Inline at the same 7 call sites, via state instead of an extension |
| **gRPC scope** | Silent on `into_router()`'s separate `axum::Router` — needs explicit follow-up either way | Same gap — threading through `router()` does nothing for the independently-built gRPC router |

Both require: extending `parse_client_ip` with hop-count awareness (per the precise,
right-to-left semantics in decision 5), and adding `ConnectInfo<SocketAddr>` extraction
somewhere in the request path (currently absent from the whole codebase — confirmed by
grep across `crates/`).

### Recommendation

**Option A, refined to A'.** The defensible version of this argument is narrower than the
original draft's: `router(db, registry, codec, auth_provider)` is a fixed 4-argument
function called from every schema's bootstrap code (confirmed at
`crates/cratestack-macros/src/include/server/axum_module/router_fn.rs`), with no options
struct to extend non-breakingly, and `RateLimitLayer`/`IdempotencyLayer` already establish
`.layer(...)`-at-startup as this codebase's idiom for exactly this class of feature
(deployment-tier, operationally tunable, safe to omit). That's sufficient justification on
its own — it doesn't need the stretched claim that the idempotency/rate-limit design doc
forbids Option B, which it doesn't say, and it shouldn't lean on "breaking = bad" as a
near-absolute given this repo's own recent practice of shipping breaking signature changes
directly pre-1.0 (#453/#454).

Within "outside `router()`," prefer **A' over the original A**: a plain
`Extension<TrustedProxyConfig>` read inline by the (already-changing)
`enrich_context_from_headers`, instead of a bespoke `TrustedProxyLayer` + `Service` +
second `Extension<ResolvedClientIp>` round-trip. Same non-breaking property, same
consumer-facing `.layer(...)` idiom, smaller and more auditable diff.

Before this goes to implementation, the design doc must additionally: (a) pin down the
hop-count algorithm as right-to-left, not left-to-right (decision 5) — otherwise the
feature doesn't actually close the spoofing gap for multi-hop chains; and (b) explicitly
scope whether the separate gRPC `axum::Router` (`into_router()`) is in or out of scope for
this change (decision 6) — silently dropping it means the acceptance criterion "Forwarded
headers ignored from untrusted peers, falling back to socket peer address" is not actually
met workspace-wide for `transport grpc` schemas.

### Implementation sketch (for the follow-up PR, not this comment)

1. **New module**, mirroring `crates/cratestack-axum/src/ratelimit/` and `.../idempotency/`
   (confirmed real precedent — both are multi-file, all comfortably under the 200-line
   ceiling: `ratelimit/layer.rs` is 129 lines, `idempotency/service.rs` is 175):
   - `crates/cratestack-axum/src/trusted_proxy/config.rs` — `TrustedProxyConfig { allowlist:
     Vec<IpAddr>, max_hops: usize }`, with `TrustedProxyConfig::none()` (default: empty
     allowlist, headers never trusted) and `::trusting(ips).max_hops(n)`.
   - `crates/cratestack-axum/src/headers/forwarded.rs` — extend `parse_client_ip` with a
     `max_hops: usize` parameter implementing the right-to-left walk from decision 5; this
     function should no longer be called unconditionally from `enrich_context_from_headers`.
2. **`enrich_context_from_headers`** (`crates/cratestack-axum/src/headers/enrich.rs`) — new
   signature accepting `Option<&TrustedProxyConfig>` and `Option<SocketAddr>` (the resolved
   peer, from `ConnectInfo`), doing the trust check and hop-count-aware parse inline;
   `None` for either input means "use the default, unauthenticated path" (peer address if
   available, otherwise no `client_ip` at all — the safe default from decision 3).
3. **Codegen** — add one combined extractor (`Option<axum::Extension<TrustedProxyConfig>>`,
   `Option<axum::extract::ConnectInfo<SocketAddr>>`) to each of the **7** generated
   handler/dispatch functions (confirmed by grep — not 8; the original draft's claim of an
   8th "missed" site double-counts `handlers_update.rs:79`, which the original triage's
   `blockers` text already included in its count of 7 via the "CRUD, list, **update**,
   procedure, subscribe-dispatch" enumeration, even though the triage's evidence list didn't
   cite it by file/line):
   - `crates/cratestack-macros/src/axum/model/handlers_crud.rs` (create, get, delete — 3
     sites, confirmed at lines 64, 150, 238)
   - `crates/cratestack-macros/src/axum/model/handlers_list.rs` (line 93)
   - `crates/cratestack-macros/src/axum/model/handlers_update.rs` (line 79 — a distinct file
     from `handlers_crud.rs`, not a missed line within it)
   - `crates/cratestack-macros/src/axum/procedure.rs` (line 112)
   - `crates/cratestack-macros/src/transport/subscribe_dispatch.rs` (line 66)
   
   The RPC (`transport rpc`) and gRPC (`transport grpc`) bindings both call into these same
   7 dispatch functions directly (confirmed — `crates/cratestack-macros/src/transport/rpc.rs`
   and `crates/cratestack-macros/src/include/server/grpc/service.rs` both invoke
   `super::axum::#dispatch_ident(...)`), so no *additional* call sites are needed for those
   transports — but see decision 6 on the gRPC router-instance gap.
4. **Docs / migration note** — new `docs/design/trusted-proxy-client-ip.md` (following the
   `idempotency-rate-limit-declarative-surface.md` template) plus a CHANGELOG entry that
   explicitly covers **both** `router()` and, if in scope per decision 6, `into_router()`:
   *"Deployments behind a reverse proxy that rely on `X-Forwarded-For`/`Forwarded` being
   recorded as audit `client_ip` must, after upgrading: (a) serve via
   `.into_make_service_with_connect_info::<SocketAddr>()`, and (b) apply
   `.layer(Extension(TrustedProxyConfig::trusting([<proxy IPs>]).max_hops(N)))` on every
   router the app serves — including the gRPC router if the schema uses `transport grpc`.
   Without both, `client_ip` will be `None` on audit events, not the proxy-forwarded value."*

### Test strategy

- **Unit, in `crates/cratestack-axum/src/headers/tests_correlation.rs`** (the existing file
  that already tests `parse_client_ip` — confirmed at lines 61/67/73 — not a new file inside
  `forwarded.rs`): extend for `max_hops` (0, 1, chain longer than `max_hops`, right-to-left
  selection per decision 5), plus new cases for the trust-check itself: trusted peer + valid
  header → header value; untrusted peer + header present → header ignored, peer address used;
  trusted peer + malformed header → falls back to peer address, no panic; no `ConnectInfo` at
  all → `None` regardless of headers.
- **Integration** — one non-PG-backed macro-integration test exercising a real generated
  router end-to-end, added to **`crates/cratestack-api/tests/`** (mirroring the existing
  `no_database_procedures.rs`'s `include_server_schema!(..., db = None)` pattern —
  `cratestack-api` has no `cratestack-sqlx` dependency under any feature, confirmed), run via
  `cargo test -p cratestack-api`, no `CRATESTACK_TEST_DATABASE_URL` dependency. The original
  draft's "`cargo test -p cratestack-axum`" is not achievable for this test as scoped, since
  `cratestack-axum` cannot invoke `include_server_schema!` at all.

---

## Reviewer notes

What changed from the original draft, and why:

1. **Corrected the call-site count from "8, not 7" back to 7.** The original draft claimed
   to have found a "missed" 8th site (`handlers_update.rs:79`) beyond the triage's count of
   7. Grepping `enrich_context_from_headers` across `crates/cratestack-macros/src/` returns
   exactly 7 real call sites (3 in `handlers_crud.rs` + 1 each in `handlers_list.rs`,
   `handlers_update.rs`, `procedure.rs`, `subscribe_dispatch.rs`); the triage's own
   `blockers` text already enumerated "CRUD, list, **update**, procedure, and subscribe-dispatch"
   as five categories summing to 7, it just didn't cite `handlers_update.rs` by file/line in
   its `evidence` array. Nothing was actually missed; the draft manufactured a discovery.
2. **Softened the precedent-doc citation.** The draft's comparison table asserted Option B
   "contradicts [the] non-goal of keeping such config off the macro-invocation/
   router-construction surface." Re-reading `docs/design/idempotency-rate-limit-declarative-surface.md`
   §7 in full: it says no such thing about macro-invocation surfaces generally — it names
   `db = Postgres` as a legitimate example of config living outside `.cstack` via a macro
   parameter. The doc's real, load-bearing distinction is `.cstack`-declarative-and-compiles-
   to-`const` vs. everything-else-imperative — both A and B stay on the imperative side of
   that line. The recommendation for A still holds, on the narrower and more defensible
   ground that `router()`'s fixed 4-arg signature has no room to grow non-breakingly, not on
   a doc-precedent argument that doesn't actually reach this deep.
3. **Rebalanced "breaking: true" as a factor.** Cited commit `32f89de` (PR #453/#454) — a
   breaking `async fn authorize` change to a public trait, shipped directly, merged the same
   day this issue was triaged — as evidence this repo doesn't treat "breaking" as
   near-disqualifying pre-1.0. The recommendation for A survives this correction (the
   argument was never *only* "non-breaking is better"), but the draft leaned on it more than
   the repo's own history supports.
4. **Flagged that `enrich_context_from_headers` breaks under both options.** The comparison
   table's "Breaking: No" for Option A was true only of `router()`'s signature, not of the
   feature's full blast radius; called this out explicitly so it isn't read as "Option A has
   no breaking surface."
5. **Added the gRPC router-instance gap (decision 6).** `into_router()` in
   `crates/cratestack-macros/src/include/server/grpc/service.rs` builds a second, separately
   served `axum::Router` for `transport grpc` schemas. Both options as originally scoped
   would leave that router's requests exactly as exposed as before the fix, silently missing
   one of the issue's acceptance criteria for that transport. Neither the original options
   table nor the migration note mentioned this; both now do.
6. **Pinned down the hop-count algorithm's direction (decision 5).** The original draft's
   phrasing ("walking up to max_hops... entries instead of always taking the first") is
   ambiguous enough to be implemented left-to-right, which would not actually fix the
   spoofing vulnerability for chains longer than one hop — the untrusted client controls the
   left end of the header. Made the right-to-left requirement explicit.
7. **Proposed A' as a lighter implementation of Option A.** A plain
   `Extension<TrustedProxyConfig>` resolved inline in `enrich_context_from_headers`, instead
   of a bespoke `TrustedProxyLayer`/`Service`/second-`Extension`-type pipeline. Same
   non-breaking property and consumer-facing idiom as the original A, smaller surface area.
8. **Corrected the test-location claims.** `crates/cratestack-axum` has no dependency on
   `cratestack-macros` and cannot invoke `include_server_schema!`, so "cargo test -p
   cratestack-axum" is not achievable for the macro-integration test as described; redirected
   it to `crates/cratestack-api/tests/`, which already has the exact non-PG pattern needed
   (`no_database_procedures.rs`). Also redirected the unit-test location for extended
   `parse_client_ip` tests to the existing `tests_correlation.rs`, matching this codebase's
   established convention of separate `tests_*.rs` files rather than inline `#[cfg(test)]`
   blocks in the module under test.
9. **Minor precision fix on the CIDR-dependency claim.** `ipnet` already resolves
   transitively via `hyper-util` (visible in `Cargo.lock`), so "no CIDR crate exists in the
   workspace" is slightly overstated — it isn't a *direct* dependency today, but adding it
   wouldn't add a wholly new crate to the dependency tree. Doesn't change the recommendation
   to defer CIDR to v2, just the strength of the stated reason.

What held up unchanged: the safe-default decision (§3), the module layout mirroring
`ratelimit/`/`idempotency/`, the overall recommendation of keeping this out of `router()`'s
signature, and the core observation that `docs/design/idempotency-rate-limit-declarative-surface.md`
supports treating this as deployment-tier operational config rather than something forced
into the macro's generated surface.

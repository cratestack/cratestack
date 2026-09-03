# A Spring-Boot-shaped application surface — spike

Status: **proposed** (2026-09-03). §4.1 is **implemented in the PR that
adds this document** — and it is smaller than first drafted: an attribute
macro was built for it, then deleted in the same PR when the break-it
check showed the language already provides the property (see §4.1, "How
this was found"). §4.2–§4.5 are proposals that wait on the decisions in
§8. No ticket exists yet — this document is the source of truth until one
does, and nothing beyond §4.1 may merge without the maintainer answering
§8.

Scope: the *user-facing* shape of a CrateStack service — what a
developer writes in `main.rs`, where behaviour goes, how configuration
and health and cross-cutting concerns are declared. Not the crate graph:
that is [`layering.md`](layering.md), and this document changes nothing
in it (§6).

> **Post-merge note (2026-09-03).** Between this document's draft and its
> merge, ADR 0015 was **accepted (amended)** and its first slice landed:
> `cratestack-exec` now occupies L3 and owns idempotency *admission*
> (#881), with rate limiting (#877) and policy replay for subscriptions
> as slices 2–3, and #880 bounded the rate-limit bucket keyspace. Nothing
> below proposes anything at L3; §4.2's builder wires the L4 adapters
> *around* the L3 executor, and §6 records that. Where the text below
> says "the two seams ADR 0012 already counts", read the post-#881 shape:
> `IdempotencyLayer` is now the thin L4 adapter over `OpExecutor::admit`.

> **What this is not.** [`layering.md`](layering.md) §6 ("Spring,
> honestly") and [ADR 0012](../adr/0012-no-ioc-container.md) already
> settled the *framework-internal* half of "make CrateStack like Spring":
> the portable ideas are Spring's seams, not its container, and the
> container is refused permanently. This document takes the other half —
> the things a Spring Boot user gets in their first hour that a CrateStack
> user currently assembles by hand — and asks, for each one, whether it
> has a compile-time twin. Where it does not, the answer is a refusal
> written down (§5), not an approximation.

## 1. The ask

"CrateStack with a structure like Spring Boot: very easy to use, yet
powerful and strongly typed like Rust."

Read as a user rather than an architect, Spring Boot's structure is
seven things:

1. **One line boots the application.** `SpringApplication.run(App.class, args)`.
2. **A named place for behaviour.** `@Service` classes with plain methods.
3. **Externalised, typed configuration.** `application.yml` bound to a
   `@ConfigurationProperties` class; profiles select overlays.
4. **Operational endpoints for free.** Actuator's health/info.
5. **Cross-cutting concerns are declared, not threaded.** Filters,
   interceptors, `@Transactional`.
6. **Test slices.** `@SpringBootTest`, `MockMvc`, `@WebMvcTest`.
7. **A project generator.** Spring Initializr.

Each is examined in §4 against one test, taken verbatim from ADR 0012:
*a collaborator is a constructor argument, a generic parameter, or one of
a small, named, hand-audited set of `Arc<dyn Trait>` seams — never a
lookup.* Anything that passes is a candidate; anything that needs a
proxy, a registry, or reflection is in §5.

## 2. Today, measured

Verified against this branch's parent (`8ee25402`).

**Booting a service is ~50 lines the framework does not write.**
`examples/microservice-pair/src/main.rs` — one model, no cross-cutting
concerns — reads two env vars by hand, opens a `PgPool`, builds a
`Cratestack`, calls the generated `router()` with **six positional
arguments** (`db, registry, resolvers, codec, auth_provider,
body_limit_bytes` — `include/server/axum_module/router_fn.rs:37`), binds
a `TcpListener`, and calls `axum::serve`. No graceful shutdown: outside
`cratestack-studio` and `examples/embedded-webhook`, nothing in the
workspace calls `with_graceful_shutdown`.

**A one-line procedure costs nine lines of signature — by convention,
not necessity.** The generated `ProcedureRegistry` method is `fn
name(&self, db, ctx, args, authorized) -> impl Future<Output =
Result<Output, CratestackError>> + Send` (`procedure/tests.rs:81-89` pins
it). That *trait-side* shape is correct — an `async fn` in a trait
cannot promise `Send`, and every axum handler needs it. But the
*impl-side* ceremony every example paid — `-> impl core::future::Future<
Output = …> + Send { async move { … } }`, 30 lines for two three-line
procedures in `examples/rpc-procedures` — is self-inflicted: since Rust
1.75 an impl may satisfy such a method with a plain `async fn`, and the
compiler checks the `Send` bound on the concrete future. Eighteen sites
across eight examples spelled the long form, `AuthProvider::authenticate`
(`core/src/context.rs`) included, and `justfile`'s `clippy_allow` carried
`-A clippy::manual_async_fn` with the rationale "examples/tests return
`impl Future` by hand" — the lint that would have said so, muted.

**The bootstrap crate exists and is not reachable.** `cratestack-service`
(#529: env-driven `ServiceConfig`, `/healthz` + `/healthz/ready`,
`telemetry::init`, `run()`) is the right idea, but: no facade re-exports
it, so a user has to know it exists; `run()` takes a finished
`axum::Router`, so the generated `router()` and the health router are
merged by the caller; and `ServiceConfig` is fixed-shape, carrying
object-storage fields absorbed from one downstream service. The only
in-workspace consumers are `cratestack-outbox` and `cratestack-migrate`'s
tests.

**Cross-cutting concerns are `.layer()` calls the user must know to
make — and, since #881, a resolver they must know to install.**
Idempotency and rate limiting are `IdempotencyLayer::new(store, ttl)` /
`RateLimitLayer::new(store, config)` applied to the router by the
application (`banking_idempotency.rs:81`, `banking_rate_limit.rs:27`).
Correct per ADR 0012 — the application chooses the adapter — but
undiscoverable: nothing at the composition root says these exist. #881
adds a second thing to know: `@no_idempotency` is honoured only if the
application also calls `.with_op_resolver(build_rest_op_resolver(
cratestack_schema::axum::ROUTE_TRANSPORTS))` (or the RPC twin over
`OPS`), and under `Router::nest("/api", ..)` the `_with_prefix` variant,
because "nothing in a request says which leading segments were the
mount". Every one of those inputs is a fact the *generated* module
already knows — the transport style, the descriptor table, and (if the
builder is told it) the mount — which is the strongest single argument
for §4.2.

**There is no project generator.** `CLAUDE.md`'s CLI list mentions
`init`; the only top-level scaffolder that actually exists is `studio
eject` (`cli_types.rs:270`). That is a documentation drift worth fixing
independently of this proposal.

## 3. The map

| Spring Boot | CrateStack today | Proposal | ADR 0012 test |
|---|---|---|---|
| Starters | The four facades (`layering.md` §4.1 — *stronger* than Spring's) | nothing; already better | n/a |
| `@Service` + plain methods | `impl ProcedureRegistry` written the long way in every example | **§4.1** write `async fn` — examples, trait docs, regression test, **shipped** | n/a — the language already provides it |
| `SpringApplication.run()` | hand-rolled `main.rs` | **§4.2** generated `App` builder + `serve()` | passes: typed builder, constructor injection (ADR 0012 names exactly this as the answer to unwieldy builders) |
| `@ConfigurationProperties`, profiles | `ServiceConfig::from_env` (fixed shape) | **§4.3** typed env config + profile overlay | passes: a struct built at startup |
| Actuator | `cratestack_service::health::router()` merged by hand | **§4.2** `.health()` on the builder | passes |
| Filters / interceptors | `IdempotencyLayer` (+ `with_op_resolver`, #881), `RateLimitLayer` applied by hand | **§4.2** `.idempotency(..)`, `.rate_limit(..)` setters; the generated builder installs the right op resolver itself | passes: explicit `Arc<dyn Store>`, the two seams ADR 0012 already counts; L3 untouched |
| `@Transactional` | `db.transaction(|tx| …)` (#513) | nothing | **fails** — needs a proxy; refused (§5) |
| `@SpringBootTest`, `MockMvc` | `router.oneshot(..)` by hand | **§4.4** typed test client | passes |
| Initializr | `studio eject` only | **§4.5** `cratestack new` | n/a (tool) |
| Auto-configuration, `@ConditionalOn*` | Cargo features (additive only) | nothing | **fails** (§5) |
| Component scan, `@Autowired` | — | nothing | **fails** (§5) |

## 4. Proposals

### 4.1 Procedures and auth providers are plain `async fn` — shipped in this PR

This is what a procedure looks like now, in every example and in the
generated trait's own doc comment:

```rust
impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn greet(&self, _db: &Cratestack, _ctx: &CratestackContext,
                   args: greet::Args, _auth: greet::Authorized)
        -> Result<greet::Output, CratestackError>
    {
        Ok(GreetReply { message: format!("hello, {}!", args.args.name) })
    }
}
```

No attribute, no framework support, no change to the trait or to the
`Authorized` witness (#512). What shipped:

- **All eighteen example sites converted** (`examples/{rpc-procedures,
  rpc-batch, rpc-batch-debounce, rpc-streaming, react-vite-swr,
  microservice-pair, no-database-verification,
  no-database-verification-api}`), procedures and auth providers alike.
- **The generated `ProcedureRegistry` trait and `AuthProvider` carry a doc
  comment** saying to implement with `async fn` and why the trait-side
  spelling differs.
- **A regression test**, `crates/cratestack-api/tests/async_fn_impls.rs`,
  drives the real generated `router()` (`db = None`) through a registry
  *and* an auth provider written as `async fn`, with an `.await` before
  the borrowed `ctx` is used. If the generated trait ever changed to a
  shape an `async fn` cannot satisfy — a boxed future, say — this file
  stops compiling.

**How this was found.** The first draft of this section was a
`#[cratestack::service]` attribute macro that rewrote `async fn` methods
into the long form — 113 lines at ⊥, five unit tests, facade re-exports,
all green. §9's break-it check (delete the attribute, expect a
trait-signature mismatch) then passed `cargo check` with exit 0: the
plain `async fn` impl compiled on its own. The macro was sugar for a
language feature; shipping it would have taught users a dependency on
the framework that does not exist. It was deleted, and the regression
test kept, in the same PR. The lesson is written into §9: the decisive
test runs *before* the artifact is trusted, not after.

**Left open.** The 36 facade test files under `crates/*/tests` still use
the long form; converting them is mechanical and is §8.1's call, together
with dropping the now-misleading `-A clippy::manual_async_fn` from
`clippy_allow` so the lint can catch regressions. `@stream` procedures
have no `async` sugar in the language either — they return `impl Stream`
and stay as they are.

### 4.2 The composition root: a generated `App` builder

The six positional arguments of `router()`, the health router, the two
cross-cutting layers, the body limit, tracing and graceful shutdown all
belong at one place a reader can look at and see the whole service. That
place should be *generated*, because it has to name this schema's
`ProcedureRegistry` and `ComputedFieldResolver` traits and this schema's
`router()`.

Sketch (names are §8's to settle):

```rust
let app = cratestack_schema::App::new(db)
    .procedures(Procedures::default())       // required unless schema has none
    .auth(HeaderAuth)                        // required — no default anonymous auth
    .codec(CodecSet::new(CborCodec, JsonCodec)) // default when `codec-json` is on
    .resolvers(())                           // default when schema has no @computed
    .body_limit(DEFAULT_BODY_LIMIT_BYTES)    // default
    .idempotency(store, Duration::from_secs(3600)) // optional: Arc<dyn IdempotencyStore>;
                                             // the generated builder adds the schema's own
                                             // op resolver (#881) — REST or RPC, it knows which
    .rate_limit(store, RateLimitConfig::new(100, 1.0)) // optional (#880's budget applies)
    .mount("/api")                           // optional; feeds the `_with_prefix` resolvers
    .health()                                // mounts /healthz, /healthz/ready
    .router();                               // a plain axum::Router — nothing hidden

cratestack_service::serve(app, &config).await?;  // bind, trace, graceful shutdown
```

Three properties, each a §8 decision:

- **Required slots are type-state, not `Option`.** Forgetting `.auth()` is
  a compile error at `.router()`, not a startup panic. This is the
  literal Rust twin of "no `NoSuchBeanDefinitionException`" (ADR 0012,
  Consequences). Cost: two type parameters on `App`; everything optional
  is a plain field with a default.
- **Optional collaborators are the existing seams, unchanged.**
  `.idempotency(..)` takes the same `Arc<dyn IdempotencyStore>` the
  `IdempotencyLayer` takes today and applies the same layer — plus the
  `with_op_resolver` call #881 made necessary, which the generated builder
  can make correctly because it knows whether the schema is REST or RPC
  and can name `ROUTE_TRANSPORTS`/`OPS`. No new trait, no new `dyn`; ADR
  0016's freeze at three operational traits is untouched. This is *not*
  L3: `cratestack-exec`'s `OpExecutor::admit` (ADR 0015 slice 1) makes
  the admission decision; the builder only composes the L4 adapters
  around it. When slice 2 moves rate limiting to L3 the builder's
  `.rate_limit(..)` setter should not need to change — that is the test
  of whether it was placed correctly.
- **`serve()` is the `cratestack-service` `run()` that exists, plus
  graceful shutdown**, reachable from the facade. Today no facade
  re-exports `cratestack-service`; an optional `service` feature on
  `cratestack-pg`/`cratestack-api` (L5 → L2, a legal edge) would make it
  `cratestack::service::serve`. `cratestack-sqlite` gets nothing — it has
  no axum.

Under `db = None`, `App::new()` takes no pool, mirroring
`Cratestack::builder()`.

### 4.3 Typed configuration and profiles

`ServiceConfig` (#529) hard-codes one downstream service's field set —
object storage endpoint, bucket, keys. The Spring shape is: the
*application* declares its config struct; the framework binds it from the
environment with a prefix, typed, failing at boot with a message naming
the variable.

Minimal proposal, no new dependency: a `cratestack_service::Env` reader
(`Env::with_prefix("ORDERS")`, `env.required::<u16>("PORT")?`,
`env.optional::<Url>("REDIS_URL")?`, `env.profile()` reading
`{PREFIX}_PROFILE`), and `ServiceConfig` re-expressed on top of it with
the object-storage fields moved out to the one service that needs them.
A `#[derive(cratestack::Config)]` on top is a later convenience, not the
mechanism. Whether to take `figment`/`config` instead is §8.5 — the
argument against is pre-1.0 dependency weight for a feature whose 95% is
"read env vars with a prefix".

### 4.4 Test support

`examples/microservice-pair`'s `router_builds_offline` test already shows
the shape: a lazy pool, a built router, `oneshot`. A generated
`cratestack_schema::test::Client` wrapping `tower::ServiceExt::oneshot`
with typed helpers (`client.procedure::<greet>(args).await`,
`client.model::<Post>().list(..)`) would replace the hand-rolled
`Request::post(..).body(..)` + `to_bytes` boilerplate every test file in
`crates/cratestack-api/tests/` currently repeats. Phase 3.

### 4.5 `cratestack new`

Initializr's twin is a CLI command, not a framework feature: `cratestack
new orders --facade pg` emitting `Cargo.toml` (the right `package =`
rename), `schema.cstack`, `src/main.rs` (§4.2's ten lines), `src/service.rs`
(an `impl ProcedureRegistry` block of `async fn`s), `migrations/`. The `studio eject`
handler already has the template machinery. Phase 3; also the moment to
reconcile `CLAUDE.md`'s phantom `init`.

## 5. Refused, and staying refused

Nothing here reopens ADR 0012. For the record, the Spring features with
no compile-time twin, and why the builder in §4.2 is not a back door to
them:

- **The container.** `App` holds concrete, caller-supplied values in
  named fields; it resolves nothing. It is the "configuration struct or
  typed builder" ADR 0012's *What would make us revisit* section names as
  the answer to growing signatures.
- **`@Transactional`.** No proxy point exists or will. `db.transaction(..)`
  (#513) is the supported shape; ADR 0018 makes it public API.
- **Auto-configuration / `@ConditionalOnClass`.** Cargo features are
  additive; defaults in §4.2 are *values* (a codec, a body limit), never
  the presence or absence of a dependency.
- **Component scanning / `@Autowired`.** Nothing is discovered. A
  `ProcedureRegistry` is a value the application constructs and passes to
  `.procedures(..)` by hand; the trait is the *place* behaviour lives,
  never a registration.
- **`@MockBean`.** A test double is a struct passed to the builder.

## 6. Layer placement

- `service_attr` — ⊥ (`cratestack-macros`); depends on `syn`/`quote`
  only. No `layers.toml` change.
- `App` — ⊥-emitted into the consumer's `cratestack_schema` module,
  exactly like `router()` today.
- `serve()` / `Env` — L2 `cratestack-service`, reached through an optional
  L5 facade feature. L5 → L2 is legal (`layering.md` §3).
- L3 (`cratestack-exec`, ADR 0015 slice 1 / #881) is untouched. Nothing
  here is an execution layer; the builder composes around it.

## 7. Phasing

| Phase | Content | Gate |
|---|---|---|
| 1 (this PR) | §4.1: eight examples converted to `async fn`; trait doc hints; regression test | none — no API change |
| 2 | §4.2 `App` + `.health()` + `serve()` with graceful shutdown; facade `service` feature; examples converted | §8.1–§8.4 |
| 3 | §4.3 `Env`/profiles; §4.4 test client; §4.5 `cratestack new` | §8.5–§8.6 |

## 8. What the maintainer must decide

1. **Finish the `async fn` sweep and un-mute the lint?** Convert the 36
   facade test files under `crates/*/tests` and drop
   `-A clippy::manual_async_fn` from `justfile`'s `clippy_allow`, so
   clippy fails the next hand-written `impl Future { async move }`. The
   rationale recorded next to the allow ("examples/tests return `impl
   Future` by hand") is half false as of this PR. Mechanical, but it
   touches every facade test crate — a separate PR.
2. **Where the composition root is generated.** ⊥-emitted `App`
   (recommended: it can name the schema's traits) vs. a library-generic
   builder in `cratestack-service` taking `|db| router(..)` as a closure.
3. **Required slots as type-state** (recommended) vs. `build() ->
   Result`. The first is a compile error, the second a boot error.
4. **Facade reachability of `cratestack-service`.** Optional `service`
   feature on `-pg`/`-api` (recommended, default-on) vs. leaving it a
   separate dependency the user must discover.
5. **Config mechanism.** Hand-rolled `Env` (recommended, no dependency)
   vs. `figment`/`config`.
6. **Scaffolder name.** `cratestack new` vs. `init` — and either way,
   `CLAUDE.md`'s CLI list needs correcting now.

## 9. Verification (phase 1)

- Regression: `cargo test -p cratestack-api --test async_fn_impls` —
  compiles a `ProcedureRegistry` and an `AuthProvider` as `async fn`
  against the real generated trait and dispatches through `router()`;
  the anonymous case proves `@allow(auth() != null)` still runs in front.
- Examples: every in-workspace converted example type-checks under
  `--all-targets`; the two out-of-workspace ones
  (`no-database-verification{,-api}`) under `--locked`;
  `examples/rpc-procedures`' `tests/smoke.rs` runs unchanged.
- Gates: `just lint`, `.ci/file-length-check.sh`,
  `.ci/changelog-check.sh`, `.ci/layer-direction-check.sh`,
  `.ci/ignore-doctest-fence-check.sh` all pass.
- The break-it check that disproved the macro (§4.1): with the attribute
  deleted from `examples/rpc-procedures`, `cargo check
  -p rpc-procedures-example` exited 0. Recorded in the PR body verbatim.

# Route suppression across REST, RPC and generated clients — spike

Status: **proposed** (2026-08-12) — awaiting accountable-owner sign-off
(@stephane-segning) per issue #514's acceptance criteria. **Not
implemented. No implementation may merge under #514** — a follow-up
ticket carries that once this is accepted or rejected.

> **Scope note (2026-08-18).** This spike originally covered a fourth
> surface, gRPC (`include/server/grpc/service.rs`), and cited its
> existing `model_allows_create` presence-gate as the one live precedent
> for declaration-gated absence (§5). `transport grpc` and that gate were
> removed in 0.8.5 — see `docs/adr/0017-remove-grpc-protobuf.md`. This
> document has been narrowed to the three surfaces that remain (REST,
> RPC, generated clients); the argument and structure for those three are
> otherwise unchanged from the original spike. Where the gRPC precedent
> was load-bearing for a specific claim, that is flagged in place rather
> than silently dropped.

> **Notation correction (2026-08-26, cratestack#743 post-merge review).**
> This document's own `@@internal("action", ...)` notation (§1, §2 below)
> reads like an `@@allow`-style call taking multiple comma-separated
> arguments in one declaration. That is not what shipped, and re-checking
> against the parser confirms it was never what PR #485 specified either
> — `cratestack_core::parse_internal_attribute` accepts exactly one
> quoted action per `@@internal(...)` declaration and hard-rejects a
> second one in the same parens (`@@internal("create", "update")` is a
> compile error naming the model, not a parse of two actions). Suppressing
> more than one action on a model means writing more than one
> `@@internal("action")` line — `@@internal("create")` on its own line,
> `@@internal("update")` on its own line — exactly the same repeated-
> declaration shape `@@allow`/`@@deny` already use for multiple rules on
> one model, and exactly what `§3.1`'s `model_internal_actions` set-union
> semantics (`BTreeSet<&str>`, order- and count-independent) already
> assumed. Every `@@internal("action", ...)` occurrence below is citing
> PR #485's original wording, not describing multi-argument syntax; read
> it as `@@internal("action")` (one action, repeat the line to suppress
> more).

Scope: a design for suppressing generated routes/dispatch-arms/client
stubs for a model action the schema author has marked unreachable from
the wire, across all three generation surfaces —
`crates/cratestack-macros/src/axum/model/routes.rs` (REST),
`crates/cratestack-macros/src/transport/rpc.rs` +
`include/server/rpc_module.rs` (RPC unary + batch), and
`crates/cratestack-macros/src/client/{rest,rpc}/model.rs` +
`cratestack-client-{dart,typescript}` (generated clients).

Tracking: #514 (this spike). Prior art: #485 (REST-only spike, closed
**DO NOT MERGE**), #486 (scoped this alongside `auth().isSystem()`,
closed with only `isSystem()` shipped), #488 (epic, Problem Statement
gap #5).

**A note on what "prior art" means here:** PR #485 was closed unmerged.
None of its code is on `main` — `crates/cratestack-core/src/schema/
internal_attribute.rs`, the per-action `MethodRouter` restructuring in
`axum/model/routes.rs`, and everything else it touched do not exist in
this repository today. This document cites PR #485's diff (via `gh pr
diff 485`) as a *design reference* to build on, per #514's own
instruction ("Do not re-derive that conclusion — build on it"), not as
code this design can assume is already present. Every "§1" claim below
was independently re-verified by reading the actual files on `main` at
the commit this branch was cut from (`ea03dc6`); every PR #485 citation
is explicitly marked as such.

## Summary

| Question | Recommendation |
|---|---|
| 1. Trigger | **Author declaration** (`@@internal("action", ...)`), not policy inference. Reintroduces PR #485's attribute design unchanged in shape (that PR's code is not on `main` — see note above); only its *effect* changes (§2, §3). |
| 2. Surfaces | **All three, one shared source of truth.** A single `model_internal_actions(&Model) -> BTreeSet<String>` — PR #485 designed this exact function in `crates/cratestack-core/src/schema/internal_attribute.rs`, but that file does not exist on `main`; the implementation ticket reintroduces it — consulted at exactly one point per surface: REST route assembly, RPC dispatch-arm collection, and every client's per-action stub emission. A suppressed op sent to `/rpc/batch` gets a per-frame `CratestackError::NotFound` error frame at its index, in place, other frames unaffected — no new mechanism needed, since this reuses machinery that genuinely is on `main` today (§3.2). |
| 3. Failure mode | Not a single status code — **each surface's own pre-existing "this dispatch key doesn't exist" fallback**, reused rather than reinvented, because suppression is implemented as *emitting nothing*, not as a new runtime branch. REST: 405 on a shared path, using axum's own default `MethodRouter` behavior once routes are restructured the way PR #485 restructured them (that restructuring is not on `main`; the underlying axum 405-for-unregistered-verb default is); or plain axum 404 if a model suppresses every action on a path. RPC: `CratestackError::NotFound` via the unknown-op-id arm that already exists on `main` today (`rpc_module.rs:116-133`) (§4). |
| 4. Client-generation consequence | **Stub absent** — a compile error for the SDK consumer. One live precedent survives: the TypeScript REST/RPC client generator already gates its `create` stub and `Create<M>Input` interface on `model_allows_create` (`cratestack-client-typescript/src/types.rs:118`, used at `context.rs:136`, `views.rs:136`, `swr/context.rs:183`) — gRPC's copies of the same check were removed with `transport grpc` in 0.8.5 (see the scope note above), but this one was never gRPC-specific. Declaration-gated absence, applied to all three client languages, all three surfaces (§5). |
| 5. Migration | **Breaking**, communicated the same way every other breaking codegen change is under this framework's pre-1.0 lockstep versioning: a minor version bump with a `CHANGELOG.md` entry naming the removed method, no deprecation window. `@@internal` is opt-in per action, so nothing breaks until an author adds it (§6). |

Rejected alternatives: policy-derived (inferred) suppression (§7.1);
a single literal `CratestackError` variant/status code forced uniformly
across all three surfaces (§7.2).

## 1. Current state, re-verified against `main`

The issue's core claim — REST model routes are unconditional — still
holds: **REST and RPC are fully unconditional. TypeScript's client
generator is the one exception** — it gates its `create` stub on
`model_allows_create`, the same partial, presence-based suppression
gRPC's now-removed copy had (§1.4). (An earlier revision of this
section also cited gRPC and its Dart/TypeScript-gRPC copies as having
this same mechanism — those gRPC-specific copies were removed with
`transport grpc` in 0.8.5, see the scope note above, but the TypeScript
REST/RPC copy was never part of gRPC and survives.) This section cites
the exact call sites so the rest of the document can reason about one
shared mechanism instead of the ones that exist today.

### 1.1 REST — fully unconditional

`generate_model_axum_routes` (`crates/cratestack-macros/src/axum/model/
routes.rs:12-35`) emits both `.route(...)` calls — `GET`+`POST` on the
collection path, `GET`+`PATCH`+`DELETE` on the detail path — for every
model, unconditionally. It is called once per model from
`collect_models` (`crates/cratestack-macros/src/include/server/collect/
models.rs:107-111`) with no policy or attribute consulted anywhere in
that path. `model_router.rs`'s `build_fn` (58 lines total, the file the
issue names) is just the merge point that folds the resulting
`model_axum_routes` tokens into one `axum::Router` — the actual
unconditional emission is one level down, in `axum/model/routes.rs`,
not in `model_router.rs` itself. Worth being precise about, since a
suppression fix belongs in `routes.rs`, not the file the issue points
at.

`generate_model_transport_constants` (`crates/cratestack-macros/src/
transport/rest.rs:39-95`) emits a `RouteTransportDescriptor` const for
all five REST routes (`list_get`, `list_post`, `detail_get`,
`detail_patch`, `detail_delete`) per model, also unconditionally. These
feed canonical-request signing and tracing, not routing directly, but
they're a second place any fix has to touch.

### 1.2 RPC — fully unconditional, both unary and batch

`generate_model_op_descriptors` (`crates/cratestack-macros/src/
transport/op_descriptors.rs:14-81`) emits all five `OpDescriptor`s
(`model.<M>.list/get/create/update/delete`) per model, unconditionally.
`generate_model_rpc_dispatch_arms` (`crates/cratestack-macros/src/
transport/rpc.rs:55-`) emits the five `op_id => { ... }` match arms
`rpc_dispatch_inner`'s `match op_id` (`crates/cratestack-macros/src/
include/server/rpc_module.rs:116-133`) dispatches on — again,
unconditionally. `/rpc/batch` (`rpc_module/batch.rs`) re-enters the
exact same `rpc_dispatch_inner` per frame (line 88), so it inherits
whatever the unary path does with no separate logic.

> **§1.3 removed (2026-08-18).** This section documented gRPC's
> `build_service`/`model_allows_create` presence-gate on `create` — one
> of two existing exceptions to "fully unconditional" anywhere in the
> pre-0.8.5 graph, and a fact that partly motivated §5's original
> "matches an existing precedent" argument. `transport grpc` was removed
> in 0.8.5 (see the scope note above), taking that gate and its
> `Code::Unimplemented` fallback with it. The other exception —
> TypeScript's own REST/RPC copy of `model_allows_create` — was never
> gRPC-specific and survives; see §1.4 and §5. (This section's number is
> retired rather than reused: §1.4 below keeps its original label
> instead of renumbering down, so a removed section and a live one never
> share a number.)

### 1.4 Clients — mixed, and the mix still doesn't cover the motivating case

- **Rust** (`crates/cratestack-macros/src/client/rest/model.rs`,
  `client/rpc/model.rs`): fully unconditional `create`/`update` methods
  on both REST and RPC client generation, no attribute or policy
  consulted at all.
- **Dart REST/RPC**: no `model_allows_create`-shaped check found
  anywhere in `cratestack-client-dart`'s REST-path builders
  (`builders_model.rs`) — unconditional, same as Rust REST/RPC.
- **TypeScript REST/RPC — the one surviving exception.**
  `model_allows_create` (`cratestack-client-typescript/src/
  types.rs:118`) gates the `Create<M>Input` interface
  (`context.rs:136`, `swr/context.rs:183`) and, via the `allows_create`
  flag it sets on `ModelApiView` (`views.rs:136`), also gates the
  generated `create` method/hook itself in every REST/RPC client
  template (`rest-client.ts.j2`, `rpc-client.ts.j2`,
  `rpc-react-query.ts.j2`, `rest-react-query.ts.j2`, `keys-rest.ts.j2`,
  `keys-rpc.ts.j2`). Same presence-only semantics as gRPC's now-removed
  copy: it returns `true` whenever *any* `@@allow("create", ...)` /
  `@@allow("all", ...)` attribute exists, regardless of what the policy
  expression evaluates to — so it does not fire for
  `auth().isSystem()`-gated `create`, the #486/#488 motivating case.

(An earlier revision of this section also listed three gRPC-specific
client copies of `model_allows_create` — Rust-gRPC, Dart-gRPC,
TS-gRPC — plus `cratestack-proto`'s own copy, as four of "five
reimplementations of one presence predicate" that motivated §3.1's
"one registry, not N independent checks" argument. All four were
removed along with `transport grpc`/`cratestack-proto` in 0.8.5. The
TypeScript REST-mode generator's own copy above is the fifth, and it
remains — the argument for one shared registry does not depend on the
exact count — see §3.1.)

Net: **REST, RPC, Rust's client, and Dart's client suppress nothing
today; TypeScript's client generator is the one partial exception.**
Like gRPC's now-removed copy, it keys off "does a create policy exist"
rather than "should this be reachable from the wire," so it does not
solve the #486 motivating case either. This is not a hypothetical gap;
it is the already-shipped state of `main` as of the commit this
document cites.

## 2. Question 1 — trigger: declaration, not inference

**Decision: an explicit author-declared attribute, `@@internal("action",
...)`, unchanged in syntax from the design PR #485 already worked out**
— that PR added `crates/cratestack-core/src/schema/
internal_attribute.rs` (`parse_internal_attribute`/
`model_internal_actions`/`INTERNAL_ACTIONS`) and a validation hook in
`cratestack-parser/src/validate/model_attributes.rs`, all confirmed
absent from `main` today (see the note under Tracking, above) — this
design proposes reintroducing that same shape verbatim. Only its
*blast radius* changes here — from REST-only to all three surfaces
(§3).

### 2.1 Why not inference

The issue frames this as inferring suppression "from policy
(`@@allow("create", false)`, or a policy no principal can satisfy)."
Both readings were checked against the current policy IR and both fail,
for different reasons:

**The literal-`false` case is not even syntactically supported today.**
`crates/cratestack-macros/src/policy/model/term.rs`'s `parse_policy_term`
— the function that turns one `@@allow`/`@@deny` term into a
`ReadPredicate` — recognizes `auth() != null`, `auth() == null`,
`auth().isSystem()`, `hasRole(...)`, `inTenant(...)`, `auth() ==
<relation>`, `<field> == <rhs>` / `!=`, and bare boolean *fields*. It
has no branch for a bare `true`/`false` literal term. A schema
literally containing `@@allow("create", false)` falls through every
branch to `find_model_field(model, "false")`
(`crates/cratestack-macros/src/policy/model/predicates.rs:99-113`),
which fails with `unknown model field \`false\` in read policy` — the
schema does not compile. (Contrast `crates/cratestack-macros/src/
policy/procedure/term.rs:35-39`, which *does* recognize bare `true`/
`false` — but that's the separate `@procedure` policy grammar, not the
model `@@allow` grammar the issue's example uses.) Confirmed further by
grep: every real `.cstack` fixture in this repo that uses a bare
`@@allow(..., true)` literal lives in `cratestack-parser`'s own
parser-only tests (which never evaluate the expression, only capture it
as a raw string — `Attribute { raw: String }`,
`crates/cratestack-core/src/schema/model.rs:134-137`) — never in an
example schema exercised through `cratestack-macros`'s real
policy-compilation path. (An earlier revision of this sentence also
cited `cratestack-proto`/`grpc_pb` fixtures that checked attribute
*presence* only; both were removed with `transport grpc` in 0.8.5 — see
the scope note above — and are no longer part of this evidence.) So even the
"decidable" half of the inference proposal would first require adding
literal-boolean-term support to the model policy grammar — a
non-trivial grammar change, not a pure codegen change, before inference
could exist at all.

**"A policy no principal can satisfy" is a general satisfiability
question, and the policy IR is not built to decide it.**
`cratestack-policy/src/read_types.rs`'s `PolicyExpr`/`ReadPredicate` are
a flat AND/OR tree over predicates that reference runtime auth-claim
values, database column values, and relation traversals — comparing an
`AuthFieldEqLiteral` against a `FieldEqLiteral` on an unrelated column,
for instance, requires reasoning about whether a real row and a real
auth claim could simultaneously take specific values, which is a
property of *data*, not of the schema. Nothing in
`cratestack-macros/src/policy/` performs constant-folding or
model-checking over this IR at macro-expansion time — every emitter in
that module (`term.rs`, `comparison.rs`, `predicates.rs`) walks the
parsed expression once and emits one `ReadPredicate` per term. Building
real satisfiability analysis would mean adding a SAT-style solver over
an open-ended, extensible predicate vocabulary (new `ReadPredicate`
variants are added routinely — `AuthIsSystem` itself shipped via #486)
— substantial new infrastructure, not a small codegen change, and one
this spike is explicitly not scoped to build (see Out of Scope in
#514).

**The motivating case makes inference actively wrong, not just hard.**
The #486/#488 workflow this feature exists to unblock is "disable the
generated `create`, supply a custom one" for a model declaring
`@@allow("create", auth().isSystem())`. That policy is *satisfiable* —
by a `SystemContext`-derived caller — so a satisfiability-based
inference would correctly decline to suppress it. But `SystemContext`
has no `From`/`TryFrom<CratestackContext>` and no constructor accepting a
caller-supplied context (`crates/cratestack-core/src/context/system.rs`,
module doc + `SystemContext::for_service`), and `CratestackContext::system` is
private and `#[serde(skip)]` (`crates/cratestack-core/src/context.rs:
25-54`) — so **no request that arrives over REST or RPC can ever
be authenticated as a system caller**. The policy is abstractly
satisfiable but concretely unreachable from the wire — a fact that
lives in `cratestack-core`'s deserialization behavior, a different crate
entirely from the policy IR, and not a property "is this policy always
false" can see. Satisfiability inference would therefore *keep* routing
the exact case #486 was filed to close. This is decisive: the trigger
this feature needs to serve its own motivating use case cannot be
inference, regardless of how much satisfiability machinery is built.

### 2.2 Why declaration earns its keep despite duplicating information

PR #485's own objection ("`@@internal` may not earn its keep... if
budget-constrained ship `isSystem()` first") was about *sequencing*, not
about rejecting the mechanism — and `isSystem()` has since shipped
(#486, closed). The duplication concern (author writes down suppression
separately from the policy that would make it moot) is real but bounded:
`@@internal`'s action vocabulary already matches `@@allow`'s
(`INTERNAL_ACTIONS = ["list","detail","read","create","update","delete",
"all"]`, mirroring the actions `@@allow`/`@@deny` accept), so there is
one new attribute line per suppressed action, not a second policy
language. It is also the only trigger that is honest about intent:
"this must never be reachable from the wire, independent of whether some
future policy edit would make it satisfiable" is a *routing* decision, a
schema author's call, not a fact deducible from the policy's current
shape.

## 3. Question 2 — all three surfaces, or none

One shared predicate, `model_internal_actions(&Model) -> BTreeSet<String>`
— PR #485 designed this function in `crates/cratestack-core/src/schema/
internal_attribute.rs`; it is not on `main` and the implementation
ticket needs to (re)add it — consulted at exactly one generation point
per surface:

| Surface | Where suppression would be checked | What "suppressed" means | Is this on `main` today? |
|---|---|---|---|
| REST | `crates/cratestack-macros/src/axum/model/routes.rs::generate_model_axum_routes` — needs the per-action `MethodRouter` restructuring PR #485 designed (`merge_method_routes`, folding surviving verbs with `.merge()` instead of one fused `.get(..).post(..)` chain). | The suppressed verb's `axum::routing::{get,post,patch,delete}(...)` call is omitted from that path's merge. If every verb on a path is suppressed, the whole `.route(path, ...)` call is omitted (`merge_method_routes` returns an empty `TokenStream` for zero survivors, per PR #485's design). | **No** — `routes.rs` on `main` still emits one fused chain (§1.1); the restructuring is PR #485-only, unmerged. |
| RPC unary | `crates/cratestack-macros/src/transport/rpc.rs::generate_model_rpc_dispatch_arms` and `crates/cratestack-macros/src/transport/op_descriptors.rs::generate_model_op_descriptors` — both take the model and would filter their per-verb `vec![...]` push against `model_internal_actions`. | The `op_id => { ... }` match arm for that verb is never emitted, so `rpc_dispatch_inner`'s `match op_id` (`rpc_module.rs:116`) falls to its `other => ...` catch-all, which already returns `CratestackError::NotFound(format!("unknown RPC op \`{other}\`"))`. The `OpDescriptor` const for that op id is also omitted, so nothing advertises the op as callable. | The catch-all arm and its `CratestackError::NotFound` **are** on `main` today (§1.2) — only the filtering-by-`model_internal_actions` step is new. |
| RPC batch | No separate change needed beyond RPC unary, above. | `rpc_batch_dispatch` (`rpc_module/batch.rs:20-114`) calls `rpc_dispatch_inner` per frame (line 88) — the exact same function unary dispatch uses, arm omission and all. A suppressed op id in a batch frame gets the same `CratestackError::NotFound`, converted to an `RpcResponseFrame::err(frame.id, &error)` at that frame's index via the existing `response_to_frame` call (line 96-98). The loop `continue`s to the next frame regardless (this already happens for any per-frame error, per the file's own module doc: "Per-frame errors don't poison the batch"). Batch order and per-frame independence are preserved with zero new code. | Every mechanism cited here **is** on `main` today — this row needs zero new code once RPC unary is done. |
| Rust client (REST) | `crates/cratestack-macros/src/client/rest/model.rs::generate_generated_model_client` | The suppressed verb's `pub async fn` is not emitted in `impl #client_ident`. | **No** gating exists today (§1.4) — fully new. |
| Rust client (RPC) | `crates/cratestack-macros/src/client/rpc/model.rs::generate_generated_rpc_model_client` | Same — suppressed verb's `pub fn` omitted. | **No** gating exists today — fully new. |
| Dart client | `crates/cratestack-client-dart/src/builders_model.rs` (REST) | Gate stub emission on the schema-level `model_internal_actions` equivalent (this is a separate crate from `cratestack-macros`; it parses `.cstack` independently via `cratestack-parser`, so it would call `cratestack_core::model_internal_actions` directly, the same public fn `cratestack-macros` would use). | **No** gating exists today (§1.4) — fully new. |
| TypeScript client | `crates/cratestack-client-typescript/src/context.rs`, `views.rs`, `swr/context.rs` | Same pattern as Dart, but replacing the existing narrower `model_allows_create` condition (§1.4) with `model_internal_actions`, extended from `create`-only to all five verbs. | `create`'s presence gate **is** on `main` today (§1.4) — extending it to all five verbs and switching its source to `model_internal_actions` is new. |

(A "gRPC" row and the "Rust client (gRPC)" row, and the gRPC halves of
the Dart/TypeScript client rows, were struck 2026-08-18 along with
`transport grpc` — see the scope note above.)

This closes exactly the gap #485 was closed for: a schema saying
`@@internal("update")` now suppresses `model.<M>.update` identically on
REST, RPC unary, RPC batch, and all three client languages — not
REST alone.

### 3.1 One registry, not N independent checks

All three surfaces read the *same* `BTreeSet<String>` per model,
computed once by `cratestack_core::model_internal_actions`. This matters
beyond tidiness: PR #485's body flagged that `model_allows_create`-
shaped presence checks were "already reimplemented four times by
convention" at the time it was written, and an earlier revision of §1.4
confirmed that to be five by the time this spike was written — four of
those five copies (three gRPC client copies plus `cratestack-proto`'s
own) were removed along with `transport grpc` in 0.8.5, leaving one
(TypeScript's REST/RPC copy, §1.4) live today, but the argument does not
depend on the exact number.
Routing every surface through one public, core-crate function (rather
than each surface's own raw-string scan of `attribute.raw`) is what
makes "all three, or none" actually checkable in review and in a future
test — a PR that adds an independent copy instead of reusing the shared
helper is a PR that visibly didn't reuse it, not a silent gap.

### 3.2 Batch semantics, stated explicitly (per #514's requirement)

A suppressed op id sent to `/rpc/batch`:

1. `rpc_batch_dispatch` decodes the batch frames normally (suppression
   is invisible at decode time — the op id is just a string).
2. For the suppressed frame, `rpc_dispatch_inner` is called exactly as
   for any other frame (`batch.rs:88`) and its `match op_id` falls to
   the unknown-op arm, returning a `CratestackError::NotFound` response.
3. `response_to_frame` (`batch.rs:96-98`) converts that into an
   `RpcResponseFrame::err(frame.id, &error)` — the per-frame error shape
   every other per-frame failure already uses.
4. The loop `continue`s; sibling frames are dispatched and succeed or
   fail independently, in the original request order (`batch.rs`'s
   existing `Vec::with_capacity(frames.len())` + push-in-order pattern,
   unchanged).
5. The batch's HTTP status stays 200 (per `docs/design/rpc-transport.md`
   §3.2: "200 if the batch parsed, regardless of per-frame outcomes").

No new logic, and this is not a hypothetical trace through the code —
`examples/rpc-batch/tests/smoke.rs::unknown_op_in_batch_returns_per_frame_not_found`
(lines 113-136 on `main` today) already exercises exactly this path for
an ordinarily-unknown op id: a batch of `[procedure.add,
procedure.does_not_exist]` asserts `status == 200`, `responses.len() ==
2`, frame 0 has no error, frame 1's error `code == "not_found"`. A
suppressed op id is indistinguishable from `procedure.does_not_exist`
at dispatch time — both are strings the generated `match op_id` simply
has no arm for — so this existing, passing test already is (without
being written for this purpose) a regression test for the suppressed-
op-in-batch behavior this section describes. (A related but distinct
test, `crates/cratestack-pg/tests/generated_client_rust_rpc.rs:195-231`
— `rpc_client_batch_per_frame_error_does_not_poison_other_frames` —
also asserts per-frame `not_found` doesn't poison a batch, but through
a *missing row* on a real, dispatchable `get` op (`get(&999)`), not an
unrecognized op id; cited here only to note it is a different code path
that happens to produce the same `code: "not_found"`, not additional
evidence for the unknown-op-id case specifically.) The implementation
ticket should add one case that uses an actually-suppressed model op id
specifically (today's `unknown_op_in_batch_returns_per_frame_not_found`
test uses a never-existed procedure name), so the assertion is pinned
to this feature rather than inferred from the pre-existing unknown-op
case.

## 4. Question 3 — failure mode

**Not one status code reused everywhere — each surface's own
pre-existing "this dispatch key was never registered" fallback wherever
one already exists on `main`, or `axum`'s own default behavior once
routes are restructured the way the design calls for** — because
suppression here is implemented as *omission at codegen time*, not as
a new runtime check that has to decide what to return:

- **REST.** Two sub-cases, both native `axum` behavior once
  `routes.rs` is restructured per §3's table (that restructuring is not
  on `main` today; the `axum::Router`/`MethodRouter` default behavior
  it relies on is `axum`'s own, not framework code, and needs no new
  code to invoke):
  - A path with at least one surviving verb (e.g. `list` stays but
    `create` is suppressed): hitting the suppressed verb gets axum's
    built-in `405 Method Not Allowed` — this exact scenario was proven
    by PR #485's own test (not on `main`; cited as evidence the
    approach works, not as something already shipped), which the PR
    body describes as "an end-to-end assertion that a real generated
    router returns 405 for suppressed verbs while a control model with
    identical policies stays routed." No `CratestackError`
    variant exists for it today (`crates/cratestack-core/src/error.rs`'s
    `CratestackError` enum has no `MethodNotAllowed`/405 case), so the 405
    body is axum's bare plain-text response, **not** a
    `CratestackErrorResponse`-shaped JSON/CBOR body. This is a real,
    named gap for the follow-up ticket (§8), not glossed over here.
  - A model suppressing every verb on a path: `merge_method_routes`,
    per PR #485's design, omits the `.route(...)` call entirely for
    zero survivors, so the path is never registered at all. No
    `Router::fallback` is configured anywhere in the axum module today
    (checked across `crates/cratestack-macros/src/include/server/
    axum_module.rs` and its submodules, and `crates/cratestack-macros/
    src/axum/` — no `.fallback(` call exists anywhere in either tree),
    so an unregistered path already falls through to axum's own default
    404 today, independent of this design — again bare, not
    `CratestackErrorResponse`-shaped.
- **RPC (unary and batch).** `CratestackError::NotFound(...)` via the
  existing unknown-op-id arm (§1.2, §3.2) — already a structured
  `CratestackErrorResponse`/`RpcErrorBody` (`code: "not_found"`), because RPC
  routing was always string-keyed dispatch through one function, so
  "unknown op id" was already a real code path with a real error type,
  unlike REST's per-path `MethodRouter`.

(A "gRPC" bullet reusing `Code::Unimplemented` via `ApiServer::call`'s
catch-all was struck 2026-08-18 along with `transport grpc` — see the
scope note above.)

**Does this satisfy "must not leak whether the model exists"?** Yes,
for the same reason in both remaining cases: a suppressed action is
indistinguishable, from the caller's side of the wire, from an action
that was simply never generated for that model — because that is
literally what suppression *is* under this design. There is no new
error path a caller could fingerprint (e.g. by timing, or by a
distinguishing error field) to tell "suppressed" apart from "never
existed," since both compile to the same absence.

**Is this "consistent," per #514's literal wording?** Not
bit-for-bit — REST returns 405 or 404 depending on path-sharing, RPC
returns 404-equivalent `CratestackError::NotFound`. Forcing one literal
status code uniformly was considered and rejected — see §7.2 for why.
The recommendation reads "consistent" as *consistent semantics*
(indistinguishable from never-generated, on every surface) rather than
*identical numeric code*, because REST and RPC do not share a
status-code vocabulary to begin with (HTTP status codes vs. RPC's own
`code: string` field already differ for the exact same underlying
condition on every other error in this framework — e.g.
`CratestackError::NotFound` is already 404 / `not_found` today, two
different literal representations of one semantic outcome).

## 5. Question 4 — client-generation consequence: stub absent

**Decision: absent.** A suppressed action gets no generated
method at all — calling it is a compile error in Rust/Dart/TypeScript,
not a runtime `403`.

> **Precedent note (2026-08-18).** This recommendation originally led
> with a "precedent already exists" argument citing gRPC's Dart/
> TypeScript-gRPC client copies of `model_allows_create`-gated
> `create`/`Create<M>Input` generation. Those gRPC-specific copies were
> removed along with `transport grpc` in 0.8.5 (see the scope note
> above). A live precedent for "absent, not deprecated" does still
> remain, though, and it was never gRPC-specific: the TypeScript
> REST/RPC client generator (`cratestack-client-typescript/src/
> types.rs:118`, used at `context.rs:136`, `views.rs:136`,
> `swr/context.rs:183`; see §1.4) omits both the `create` method/hook
> and the `Create<M>Input` interface on the same `model_allows_create`
> predicate. It only covers one action (`create`) on one client
> language, gated by presence rather than by declaration, so it does
> not by itself establish the full shape this recommendation proposes.
> The recommendation is unchanged, but it now rests primarily on the two
> arguments below, with this narrower precedent as secondary support.

Why, argued rather than asserted:

- **A compile error is discoverable at the right time.** The issue's own
  framing of the current bug — "callers discover it is dead only at
  runtime, via `403`" — is precisely the failure mode a present-but-
  deprecated stub still has: a deprecation warning is silence-able,
  ignorable, and CI-invisible unless a consumer's lint config specifically
  fails on it (most don't, by default, across Rust/Dart/TS's three very
  different deprecation-warning severities). A missing method is not
  silence-able in any of the three languages — the build fails.
- **Pre-1.0 lockstep versioning removes the usual argument for
  deprecate-then-remove.** Deprecation windows exist to give consumers
  time to migrate across a boundary where old and new servers must
  coexist. This framework's public crates version together off one
  workspace `version`
  (`CLAUDE.md`: "the public crates are versioned together off the
  workspace `version`"), and it is pre-1.0 — every minor bump is already
  understood by consumers as a potentially-breaking regeneration, not a
  guaranteed-additive one. A deprecation phase would import a stability
  promise this framework does not make anywhere else in its codegen
  surface today.

## 6. Question 5 — migration

**Breaking**, for any consumer whose generated client currently calls a
method on an action an author newly marks `@@internal`. Concretely: a
consumer regenerates their client after a schema author adds
`@@internal("update")` to a model the consumer's code calls
`.update(...)` on — their build breaks at the call site, not at
runtime.

How it's communicated, given lockstep pre-1.0 versioning (no server/
client version negotiation exists in this framework — a schema and its
generated clients are regenerated together from one source of truth):

- **It is opt-in per action, so nothing breaks silently on an upgrade
  that touches unrelated schema.** `@@internal` is an attribute an
  author adds deliberately to one action on one model; a consumer only
  hits this if the schema author chose to suppress an action their code
  actually calls. This is different in kind from e.g. a codec change
  that breaks every consumer regardless of what they use — it is closer
  to deleting a field, a category of change this framework already
  treats as an ordinary breaking schema edit, not a special migration
  case.
- **Communicated the same way every other codegen-breaking schema
  change is: `CHANGELOG.md` + a version bump.** No new migration
  machinery is proposed here. The implementation ticket's PR adds a
  `CHANGELOG.md` entry naming the action and model; `just bump 0.x.y`
  moves the workspace version; consumers regenerate and get a compile
  error pointing at the exact call site, which is more actionable than
  the current state (a `403` discovered by a human at runtime, possibly
  in production).
- **No deprecation window, matching §5's reasoning** — pre-1.0 lockstep
  versioning does not promise one anywhere else in this framework's
  generated surface (e.g. a model field rename or removal is already an
  ordinary breaking regeneration today, no soft-landing phase).
- **What this document does not decide:** whether `cratestack-lsp` or
  `cratestack-cli check` should warn when a schema edit newly suppresses
  an action a *sibling* schema's client generation config still targets
  (multi-schema consumers). That is a real usability question but not
  one of #514's five required questions, and is left as an open item
  for the implementation ticket (§8).

## 7. Rejected alternatives

### 7.1 Rejected: policy-derived (inferred) suppression

Covered in depth in §2.1. Summarized: the literal-`false` case does not
parse under the current model-policy grammar (a prerequisite grammar
change, not a codegen change); the general "no principal can satisfy"
case is a data-dependent satisfiability question the policy IR has no
machinery to decide and building that machinery is out of this spike's
scope; and the feature's own motivating case
(`auth().isSystem()`-gated `create`) is *satisfiable* in the abstract
sense, so satisfiability-based inference would fail to suppress the one
scenario #486 was filed to unblock. Declaration was chosen instead
because it is a schema-author routing decision that cannot be recovered
from the policy expression alone.

### 7.2 Rejected: one literal `CratestackError`/status code forced uniformly

Considered: add a new `CratestackError::Suppressed` (or reuse `NotFound`
everywhere) and thread it through all three surfaces so
every binding returns byte-identical status/code output for a
suppressed action. Rejected because:

- REST's per-path `MethodRouter` structure makes a uniform "path
  doesn't exist" 404 impossible to produce for a single suppressed verb
  without either (a) hand-rolling method dispatch instead of using
  axum's own routing (throwing away #485's already-tested `.merge()`
  design for no behavioral gain), or (b) adding middleware that
  intercepts axum's automatic 405 and rewrites it — real, deferrable
  implementation cost with no caller-visible benefit, since a 405 on a
  known path and a 404 on an unknown one are already both "this
  operation is not available here" from a caller's perspective.
- The framework's own existing error-mapping conventions already
  establish that "one semantic outcome, more than one literal wire
  representation per binding" is the framework's standing convention,
  not an exception — `CratestackError::NotFound` is already 404 /
  `"not_found"` under the exact same reasoning this document applies to
  suppression. Insisting on byte-identical output specifically for
  suppression would be a new, narrower consistency bar than the rest of
  the framework holds itself to.

(A third bullet arguing gRPC's `Code::Unimplemented` shouldn't be forced
to `Code::NotFound` was struck 2026-08-18 along with `transport grpc` —
see the scope note above.)

## 8. Left for the implementation ticket, not decided here

- Whether REST's 405/404 responses for a suppressed action should gain
  a `CratestackErrorResponse` body (currently bare axum text) to match
  RPC's already-structured error shape — flagged in §4, not resolved,
  since it is implementation work (new middleware or fallback handler),
  not a design question with more than one reasonable answer once §4 is
  accepted.
- A regression test pinned to an *actually-suppressed* op id inside a
  `/rpc/batch` frame, as opposed to the pre-existing
  `unknown_op_in_batch_returns_per_frame_not_found` test §3.2 cites,
  which covers the same code path via a never-defined procedure name
  rather than a genuinely suppressed one — this spike verified the
  behavior by reading the code and the existing test, not by adding a
  new one, per the constraint against implementation work under #514.
- Whether `cratestack-lsp`/`cratestack-cli check` should surface a
  warning when `@@internal` is added to an action a generated client
  config elsewhere in the workspace still calls (§6, last bullet).
- The exact `CHANGELOG.md` wording convention for a suppression-caused
  breaking change — left to whichever PR implements the first real
  `@@internal` suppression, so it can point at a concrete example.

## 8a. Known-incomplete surfaces after implementation (added 2026-08-26)

One surface was checked during cratestack#743's implementation review
and found to still emit something for a suppressed verb. It was traced
end-to-end and confirmed **not** to be a leak — documented here,
deliberately, rather than silently left for the next person to
re-discover by reading generated output.

- **TypeScript `swr` preset's cache-key factories**
  (`crates/cratestack-client-typescript/templates/src/swr/
  keys-rest.ts.j2`, `keys-rpc.ts.j2`). `list`/`get`/`update`/`delete`
  key-factory functions (e.g. `swrKeys.model.<M>.update(id)`) are
  emitted unconditionally, with no `{% if %}` gate at all — unlike
  `create`'s factory, which *is* correctly gated
  (`{% if model.allows_create %}`, and `model.allows_create` is itself
  `model_allows_create(model) && !internal.contains("create")` as of
  this ticket, so a suppressed `create` already loses its key factory).
  Confirmed **not a leak** for the remaining four: these are inert
  array-tuple literals (`["<route>", "update", id] as const`) with no
  network behavior of their own — SWR only fetches when a hook actually
  calls `useSWR`/`useSWRMutation` with one. The *hooks* that would
  actually issue a request (`models-hooks-rest.ts.j2`,
  `models-hooks-rpc.ts.j2`) are correctly gated per verb on
  `model.allows_list`/`allows_get`/`allows_update`/`allows_delete`
  (`cratestack-client-typescript/src/views.rs`, all sourced from
  `model_internal_actions` as of this ticket) — so a suppressed verb's
  key factory exists but is orphaned: nothing generated ever calls it.
  This is deliberate, not an oversight: gating four key factories per
  model for a purely-cosmetic reason (a function reference that's never
  called still typechecks and tree-shakes away) wasn't judged worth the
  added branching in the template, especially since `create`'s factory
  is the one case where leaving it ungated would have been
  user-visible (its presence used to signal "you can create this" to a
  reader of generated output, before this ticket). **What would have to
  change if this ever became a real leak:** if a future preset or
  hand-written consumer ever calls a key factory *directly* (bypassing
  the generated hook) to seed a cache entry speculatively — e.g.
  `mutate(swrKeys.model.Widget.update(1), someValue)` without ever
  calling the network layer — the key would exist and look valid with
  no corresponding request ever having been rejected, which could mask
  the suppression from a developer reading the cache rather than the
  network tab. If that pattern becomes real, gate `list`/`get`/
  `update`/`delete` on `model_internal_actions` the same way `create`'s
  factory and all five hooks already are.

(The `RouteTransportDescriptor`/`ROUTE_TRANSPORTS` gap this section
originally also listed — `crates/cratestack-macros/src/transport/
rest.rs` emitting a const for every verb unconditionally — was fixed
during the same review round rather than documented as known-incomplete:
§1.1 above already named it as "a second place any fix has to touch",
so leaving it unfiltered was closing a gap the design itself called
out, not a new judgment call. `generate_model_transport_constants` and
`generate_model_transport_entries` now consult `model_internal_actions`
exactly like the other four surfaces.)

## 9. Non-goals (carried over from #514)

- Changing policy evaluation semantics — `@@internal` is purely a
  codegen marker; a suppressed action's `@@allow`/`@@deny` rules still
  compile and still gate server-side (procedure) calls through the ORM,
  exactly as PR #485 designed it.
- The other #488 gaps (declarative custom query, `db.transaction()`
  combinator, procedure policy bypass) — each has its own ticket.
- Any implementation. This document is the deliverable; #514's
  acceptance criteria require this design to be reviewed and accepted
  or rejected by @stephane-segning before an implementation ticket is
  opened.

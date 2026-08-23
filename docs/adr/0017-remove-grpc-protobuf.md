# ADR 0017: Remove gRPC/Protobuf Support

## Status

Accepted

> **Correction (2026-08-23):** this ADR was written anticipating that the removal would
> ship as v0.9. It in fact shipped in **0.8.5** (cratestack#655) — no v0.9 release was
> ever cut. The "effective v0.9" language below is left as written, as a record of the
> decision at the time it was made; treat "0.8.5" as the accurate release for any
> reference elsewhere in the codebase (see cratestack#654).

## Date

2026-08-13 (decision); 2026-08-18 (implementation, this PR)

Context doc: none — this PR deletes the feature's design document,
`docs/design/protobuf.md`; see "Supersedes" below.

## Context

`transport grpc` shipped as a third transport option alongside REST and RPC: a `.cstack`
schema could declare `transport grpc` instead, generating `.proto` messages (with a
field-number lockfile via the `@pb(N)` attribute), a tonic service, and Rust/Dart/
TypeScript(gRPC-Web) clients. It covered model CRUD and unary/server-streaming
`procedure`s. Four `publish = true`/documentation-facing files marked it "Planned for
removal in v0.9": root `README.md`, `crates/cratestack-grpc/README.md`,
`crates/cratestack-macros/README.md`, and `crates/cratestack/README.md`. Neither
`docs/design/layering.md` nor any ADR in the 0011–0016 series carried that marker — this
PR is that removal, made concrete after the maintainer's 2026-08-13 decision to drop the
surface entirely rather than continue investing in it.

The surface was large and cross-cutting. It touched two dedicated crates
(`cratestack-grpc`, `cratestack-proto`), a `grpc` Cargo feature threaded through
`cratestack-pg`, `cratestack-client-rust`, and `cratestack-macros`, three codegen
directories inside `cratestack-macros` (`include/server/grpc/`, `include/grpc_pb/`,
`include/client/grpc/` — the Rust gRPC client *generator*), a grpc-specific module in
each of the two client *generators* that are separate crates
(`cratestack-client-{dart,typescript}`), a grpc-specific *runtime* module in
`cratestack-client-rust` (`src/grpc/`, the hand-written `tonic`-based SDK the
macros-generated Rust gRPC client called into — not a generator itself), an
unconditionally-compiled `grpc_bridge.rs` in `cratestack-axum`, the `transport grpc`
schema keyword, the `@pb(N)` field-number attribute, the `generate-proto` CLI subcommand,
and one full example (`examples/grpc-widgets/`). Layer placement lived in
`docs/adr/layers.toml`, not repeated across all six layer-model ADRs: `cratestack-grpc`
was placed at L4 and `cratestack-proto` at L1 there. Of the ADR prose itself, only
0011 (introducing the layer model) and 0014 (layer-direction enforcement) mention
either crate by name — 0011 cites both in its illustrative layer table, and 0014 cites
`cratestack-proto` repeatedly as a worked example for the "layer number, not a role"
argument; 0012, 0013, 0015, and 0016 mention neither. The surface was still cited
repeatedly as a worked example elsewhere: the "second router instance" wiring hazard
`trusted-proxy-client-ip.md` and ADR 0015 both reasoned about, the
client-codegen-deduplication precedent `docs/adr/0013` and `docs/adr/0016` cited, and
the one crate whose commit-size ADR 0015 used to bound how expensive adding a transport
binding can get.

Continued investment in the surface competed directly with REST and RPC, the two
transports every other part of this framework — policy, idempotency, rate limiting,
audit, the RPC batch envelope, the generated client SDKs — is designed and tested
against. gRPC's own generated surface reused the same underlying dispatch functions as
REST/RPC (§1 of the `route-suppression.md` spike documented this in detail)
but required its own router construction, its own field-number lockfile, its own
per-language client module, and its own presence-based suppression logic that
`route-suppression.md` found did not even solve gRPC's own motivating case. The
maintainer judged that cost no longer justified the surface it protected.

## Decision

**Protobuf/gRPC support is removed from CrateStack, in full, effective v0.9.** No
deprecation window, no feature-flagged fallback, no partial retention of the wire
format or codegen. Concretely, this PR (and five other agents working the same
decision in parallel on disjoint file sets) removes:

- The `cratestack-grpc` and `cratestack-proto` crates, in their entirety.
- The `grpc` Cargo feature from `cratestack-pg`, `cratestack-client-rust`, and
  `cratestack-macros`.
- Three codegen directories inside `cratestack-macros`: `include/server/grpc/`
  (service/router generation), `include/grpc_pb/` (`.proto` message/enum emission and
  the field-number lockfile), and `include/client/grpc/` (the generated Rust gRPC
  client).
- The grpc-specific client generator modules in `cratestack-client-dart` and
  `cratestack-client-typescript`, and the grpc-specific runtime module (`src/grpc/`,
  the hand-written `tonic` client SDK — not a generator) in `cratestack-client-rust`.
- `cratestack-axum`'s `src/rpc/grpc_bridge.rs`, which was compiled unconditionally
  rather than behind the `grpc` feature.
- The `transport grpc` schema keyword and its parser/semantic-checker support.
- The `@pb(N)` field-number attribute.
- The `generate-proto` CLI subcommand.
- `examples/grpc-widgets/`.
- `docs/design/protobuf.md` (superseded by this ADR — see below) and
  `docs/design/grpc-codegen-deduplication.md` (an unimplemented proposal for the surface
  being removed, now moot).

A schema that declares `transport grpc` or uses `@pb(N)` after this PR fails to parse.
There is no compatibility shim. This is a breaking change to any consumer that shipped a
`transport grpc` schema, communicated the same way every other breaking codegen change is
under this framework's pre-1.0 lockstep versioning: a `CHANGELOG.md` entry, no
deprecation phase.

REST and RPC are unaffected. Nothing in this decision touches either transport's
grammar, dispatch, or generated clients.

## Consequences

### Positive

- Two fewer crates, one fewer Cargo feature threaded through three others, and one
  fewer codegen surface each client generator has to keep in sync with the schema IR.
- The layer-model ADRs (0011–0016) lose their most complex worked example — the
  "second router instance" hazard `trusted-proxy-client-ip.md` and ADR 0015 reasoned
  about only existed because gRPC ran a raw `tonic::Service` alongside axum instead of
  extending the one router REST/RPC already share. That specific hazard class no
  longer has a live instance in this codebase.
- `cratestack-client`'s "no `grpc` feature, so `axum` can't leak in transitively"
  argument (`.github/workflows/ci.yml`'s `facade-disjointness` job) simplifies to "no
  feature pulls `axum` in at all" — there is no longer a `grpc` feature anywhere in the
  graph to reason about.

### Negative

- Any consumer running a `transport grpc` schema has a hard breaking change with no
  migration path other than moving to REST or RPC and regenerating every client.
- Design documents that used gRPC as a worked example (`docs/adr/0015`'s "the corollary
  matters as much as the rule" section, most notably) lose that example without a
  faithful substitute — no other surface in this codebase has the same shape. Those
  sections are amended in place with a dated note rather than backfilled with a weaker
  analogy.
- The `route-suppression.md` spike (#514, still awaiting sign-off) loses one of its
  four generation surfaces and is narrowed to three. Its client-generation
  recommendation (§5, "stub absent") is unchanged: the precedent it cited *survives*
  the removal, because only gRPC's copies of the `model_allows_create` presence gate
  were gRPC-specific — the TypeScript REST/RPC generator's own copy
  (`cratestack-client-typescript/src/types.rs:118`, used at `context.rs:136`,
  `views.rs:136`, `swr/context.rs:183`) was never tied to `transport grpc` and is
  still live. What the spike loses is breadth of precedent, not the precedent itself.

### Deferred

None. This is a full removal, not a phased one. Revisiting it — reintroducing gRPC or
a comparable binary RPC transport — would be a new ADR, not a reversal of this one.

## Supersedes

This ADR supersedes `docs/design/protobuf.md` (566 lines, the feature's original design
document), deleted by this PR. Anything in that document not already carried forward
into `docs/design/layering.md`'s historical record or the ADRs listed above no longer
applies.

## Alternatives considered

**Keep gRPC behind a feature flag, off by default.** Would preserve existing consumers'
schemas without forcing an immediate migration. Rejected: the surface's cost was never
in whether it compiled by default, but in the codegen, review, and design-document
maintenance burden of keeping four generation surfaces (REST, RPC, gRPC, clients)
mutually consistent — a cost paid whether or not the feature is enabled for any given
build. A flag would have kept paying it.

**Deprecate for one release cycle before removing.** This framework's public crates
version together off one workspace `version` and make no additive-only promise across
minor bumps pre-1.0 (see `docs/design/route-suppression.md` §5's identical reasoning for
a smaller-scoped removal). A deprecation phase would import a stability promise this
framework does not make anywhere else in its codegen surface.

**Narrow gRPC instead of removing it (e.g. model CRUD only, drop streaming).** Rejected
on the same competing-investment ground as full removal: a narrower gRPC surface still
requires its own router, lockfile, and per-language client modules — the fixed cost
that motivated removal, not the variable cost proportional to feature scope.

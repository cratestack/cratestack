# ADR 0018: CrateStack as an ORM is a supported posture

## Status

Accepted

> **Placement note.** `docs/adr/README.md` splits the series by audience: decisions
> about the *internal shape* of the workspace live here, user-visible surface decisions
> live in `cratestack-docs/internals/`. This one sits on the seam — it decides which
> public items carry a compatibility commitment, which is a workspace-shape question,
> but the thing being committed to is user-facing API. Filed here because that is where
> cratestack#488's decision comment asked for it ("Recorded as an ADR in the
> implementation PR") and because it names crate-internal call paths throughout. Move it
> if the maintainer reads the split the other way.

## Date

2026-09-02 (decision, [cratestack#488 comment][decision]); this PR (record)

Context: epic [cratestack#488][epic], open question 2. Design docs for the four
features it draws on: `docs/design/route-suppression.md`,
`docs/design/declarative-custom-query.md`.

## Context

Epic cratestack#488 was opened because a prospective adopter declined CrateStack on the
grounds that it does not remove the database layer, it *adds* one — you still write and
maintain `sqlx` alongside it. Investigating that claim found it narrower than stated but
not wrong: the generated data layer was reachable ergonomically only through the
HTTP/RPC surface, and every gap outside that surface pushed the caller back onto raw
`sqlx`.

The underlying question the epic raised, and left open as its open question 2, is not
about any one gap. It is whether "use CrateStack as your data layer, from code that is
not serving an HTTP request" — a cron job, a background worker, a custom write path,
admin tooling — is a **supported posture with a compatibility commitment**, or merely an
accident of which items happen to be `pub`. The distinction is not academic: it decides
whether a downstream service can build on `invoke_with_db`, `db.transaction(...)` and
the `ProcedureRegistry` trait, or has to treat them as internals that may move without
notice.

Four features have now shipped whose *only* purpose is non-request-scoped use, each with
its own ticket, design doc and acceptance bar:

- **`auth().isSystem()`** (cratestack#486) — a fail-closed named principal for a caller
  with no request behind it, replacing the self-asserted-claims workaround. A background
  job asserting `role: admin` used to be indistinguishable from a real admin.
- **`db.transaction(...)`** (cratestack#513) — composing several writes atomically
  without a `sqlx::Transaction` in the caller's signature or `sqlx` in its manifest.
- **`@@internal("action")`** (cratestack#743) — a model action reachable in-process but
  absent from every wire surface and every generated client.
- **The declarative `query` block** (cratestack#867, this PR) — a typed, parameterized,
  policy-checked raw-SQL read for the queries the generated builders cannot express.

Each was accepted on its own merits. What none of them individually settled is whether
the posture they add up to is one the project stands behind.

## Decision

**Yes. Using CrateStack as an ORM — as the data layer for code that is not serving an
HTTP request — is a supported posture, not an accident.**

Concretely, that commitment means:

1. **The in-process call surface is public API** under the workspace's ordinary pre-1.0
   compatibility posture (lockstep versioning, breaking changes get a minor bump and a
   changelog entry naming what moved). This covers, at minimum: the generated
   `Cratestack` handle and its model/view/query accessors, `db.transaction(...)`,
   `db.pool()`, each procedure module's `authorize_with_db`/`invoke_with_db`, the
   `ProcedureRegistry` trait, and each `query` block's generated `run`.
2. **A gap in that surface is a bug, not a "use `sqlx`" answer.** "You can always drop
   to `db.pool()`" stops being a sufficient response to a report that an ordinary data
   need has no expression in the framework. `db.pool()` remains available and
   deliberately so — it is the escape hatch of last resort, not the plan.
3. **Policy applies to in-process callers exactly as it does to wire callers.** This is
   the part that makes the posture safe rather than merely convenient, and it is already
   enforced structurally: cratestack#512's unconstructible `Authorized` witness makes
   the policy-skipping `registry.method(&db, &ctx, args)` call shape fail to compile,
   and cratestack#867's `query` blocks check `@allow` unconditionally inside their
   single generated entry point with no unchecked twin to reach for.

### What this does *not* decide

- **It does not promise feature parity between the in-process and wire surfaces.** A
  `query` block is in-process-only by design; `@@internal` actions are wire-absent by
  design. The commitment is that the in-process surface is *supported*, not that it
  mirrors the wire surface.
- **It does not weaken the transport-parity rule for wire features.** Anything that
  touches the request/response surface still ships on REST and RPC together (see
  `CLAUDE.md`). This ADR is about a third surface, not an exemption from that one.
- **It does not make raw SQL safe by association.** A `query` block's SQL gets no
  soft-delete filtering and no row-level `@allow` injection — the author owns every
  predicate. See `docs/design/declarative-custom-query.md` §6.

## Consequences

**Positive.**

- A downstream service can state, and check, that it has no direct `sqlx` dependency.
  Two CI-enforced example crates make that concrete rather than aspirational:
  `examples/db-transaction-verification` (cratestack#513) and
  `examples/declarative-query-verification` (cratestack#867), both re-run by the
  `facade-disjointness` job on every PR.
- Reports of the form "I had to reach for `sqlx` to do X" now have a home. Before this
  decision, whether such a report was a bug or working-as-intended depended on who read
  it.
- The compatibility commitment is stated once, here, rather than inferred per-item from
  whether something happens to be `pub`.

**Negative, and accepted.**

- **A wider public surface is a wider surface to keep stable.** `invoke_with_db`'s
  signature, the `ProcedureRegistry` trait's shape and the generated accessor names are
  now things that cannot change silently. cratestack#512 is the precedent for how that
  goes when it must: a real breaking change, a minor bump, and a changelog entry naming
  the exact migration.
- **In-process use has no HTTP boundary to lean on**, so there is no request-scoped
  authentication step in front of it. `auth().isSystem()` exists precisely because the
  honest answer to "who is this caller?" had to be a real named principal rather than
  whatever the caller asserted. Any future in-process capability has to answer that
  question the same way.
- **Two surfaces can drift.** A feature that lands on the wire path and not the
  in-process one (or the reverse) is now a real inconsistency rather than a
  non-question. There is no CI check for this and none is proposed — it is a review
  concern, named here so it is not discovered as a surprise.

## Alternatives considered

**Leave it undecided.** The status quo since the epic was filed on 2026-08-09. Rejected
because it is not actually neutral: with no stated posture, each of the four shipped
features reads as a one-off convenience, and the next "do I have to use `sqlx` for
this?" report has no principled answer. The epic's own Strategic Intent already said
"using CrateStack as an ORM should be a supported posture, not an accident of which
items happen to be `pub`" — declining to record it would leave that as an unratified
aspiration in an epic body while four features shipped on its strength.

**Say yes, but without a compatibility commitment.** "Supported, but these items may
move" — cheaper to maintain, and honest about pre-1.0 churn. Rejected because it does
not answer the question anyone is actually asking. An adopter deciding whether to build
their write path on `invoke_with_db` needs to know whether it will still be there next
minor, and "supported" without that is a word rather than a commitment. The pre-1.0
posture already permits breaking changes with a bump and a changelog entry; that is the
right amount of freedom, and it is what item 1 above commits to.

**Ship a separate "ORM mode" facade.** A fifth facade crate exposing only the
in-process surface, disjoint from the axum/wire surface the way `cratestack-client` is
(ADR 0013). Rejected as solving a problem nobody has: unlike the client/server split,
in-process and wire callers live in the *same* process and want the *same* `Cratestack`
handle — a service's background worker and its HTTP handlers share one connection pool
and one schema. A fifth facade would duplicate the surface without removing anything
from anyone's dependency graph, which is the only thing the facade split exists to do.

[epic]: https://github.com/cratestack/cratestack/issues/488
[decision]: https://github.com/cratestack/cratestack/issues/488#issuecomment-5514756770

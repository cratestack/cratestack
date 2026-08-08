# ADR 0014: Mechanical Enforcement of Downward-Only Dependencies

## Status

Accepted

## Date

2026-08-08 (amended 2026-08-08 — see "Amendment" note below)

Context doc: [docs/design/layering.md](../design/layering.md)

**Amendment.** This ADR was drafted claiming no single integer works for `cratestack-macros`
(see the original "⊥ has no number" bullet, corrected below) and left three questions
explicitly to the maintainer. Both are now resolved, in the same PR that lands the checker
(`.ci/layer-direction-check.sh`, `docs/adr/layers.toml`): the "no integer works" claim does
not survive verification against the real dependency graph, and the three open decisions in
the original "What the maintainer must decide" list are answered below. Status moves to
Accepted on that basis — not because every negative consequence has evaporated (the
`cratestack-core` blind spot and the dev-dependency hole are unchanged and still listed under
Consequences), but because the three genuine trade-offs this ADR existed to resolve are now
resolved, and the checker exists, runs, and demonstrably catches the #475 violation it was
built to catch.

## Context

`layering.md` §3 states the rule: *a crate at layer N may depend on crates at layers ≤ N,
and on nothing at a layer > N; same-layer edges are legal provided the intra-layer graph
stays acyclic.* Nothing checks it.

The rule is not decorative. PR #465 (`6f14f1e`, "fix(cratestack-core,cratestack-sql): move
storage traits and DDL from cratestack-axum to core/sql") fixed a real back-edge:
`cratestack-sqlx` and `cratestack-redis` both listed `cratestack-axum` in
`[dependencies]` — two storage adapters downstream of the HTTP transport crate — for no
reason other than that `IdempotencyStore` and `RateLimitStore` had been *defined* there.
The commit message records how it was found and proven: `cargo tree -p cratestack-sqlx -i
cratestack-axum` → no match. A human ran that command. Nothing else would have.

The only dependency gate in `just all-checks` is `cargo deny check` (`justfile:43`). Its
entire `[bans]` section in `deny.toml` is `multiple-versions = "warn"`. `cargo-deny`
validates licences, advisories, bans and sources; intra-workspace direction is not a thing
it models and never will be.

**The mechanism is cheap and already present in this repo, twice.** `ci.yml`'s `check` job
already runs `cargo metadata --locked --format-version=1` as its first step (line 31,
before `rust-cache`, deliberately — see the comment at line 27). `justfile`'s
`release-publish` recipe already pipes `cargo metadata --format-version=1 --no-deps`
through an inline `python3` script (lines 627–645) that builds exactly the graph this ADR
needs — `{d["name"] for d in p["dependencies"] if d["name"] in pkgs and d["name"] != n}` —
and topo-sorts it, precisely because a hand-maintained publish order had already drifted
once. A direction check is that script plus a table lookup and one comparison. It compiles
nothing. Verified: `cargo metadata --no-deps` reports `kind: "dev"` on dev-dependencies,
`kind: "build"` on build-dependencies and `kind: null` on normal ones, so the three are
separable in one key.

**The hard part is the table, not the mechanism.** Encoding `layering.md` §2's assignment
and ADR 0011's completion of it, then running it against the `[dependencies]` graph at
`origin/main` (`08fbb7e`), produces five findings on day one, and they are not the same
kind of thing:

| Edge | Under the model | Verdict |
|---|---|---|
| `cratestack-client-store-sqlite → cratestack-client-rust` | L2 → L4 | **Real. Unfixed.** |
| `cratestack-client-store-redis → cratestack-client-rust` | L2 → L4 | **Real. Unfixed.** |
| `cratestack-sql → cratestack-policy` | L1 → L1 | Legal under `≤`; a violation only under the withdrawn `<` form |
| `cratestack-client-flutter → cratestack-client-rust` | L4 → L4 | Same |
| `cratestack-cli → cratestack-studio` | tool → tool | Table artifact — `layering.md`'s "tools may depend on anything and be depended on by nothing" is self-contradictory for tool→tool edges |

The first two matter. `pub trait ClientStateStore` is defined at
`crates/cratestack-client-rust/src/state.rs:43`, and two storage adapters depend on the
binding crate solely to implement it — structurally the same shape as the pre-#465
`IdempotencyStore`-in-`cratestack-axum` defect, on the client half of the graph, still
there. `layering.md` §2 names these crates "the client-side instance of the same pattern"
and files them at L2; nobody had noticed they point upward, because noticing required
enumerating edges mechanically. That is the falsification of "vigilance is enough," and it
was produced by prototyping this check.

Three structural complications the checker must handle, none of which are optional:

- **Target-gated sections.** `cratestack-sqlite → cratestack-client-rust` lives under
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`
  (`crates/cratestack-sqlite/Cargo.toml:63-64`). `cargo metadata` reports it with
  `kind: null` and a non-null `target`. It is L5 → L4 and legal, but a checker reading
  only the plain `[dependencies]` table of each `Cargo.toml` would miss it entirely, and a
  checker that treats `target != null` as "skip" would create a permanent blind spot.
- ~~**⊥ has no number.**~~ **Corrected — this claim was false and does not survive
  verification.** The original draft asserted that no integer satisfies both "L5 depends on
  `cratestack-macros`" and "`cratestack-macros` must not depend on L2," reasoning "anything
  low enough to forbid `macros → sqlx` is too low for `pg → macros`." That reasoning is
  wrong: the rule is `dep.layer ≤ self.layer`, so a *low* number on `cratestack-macros` makes
  it *easier*, not harder, to satisfy `pg → macros` (any number ≤ 5 works), while
  simultaneously being exactly what forbids a hypothetical `macros → sqlx` edge (which would
  need `sqlx.layer(2) ≤ macros.layer`, i.e. `macros.layer ≥ 2` — false once macros is ≤ 1).
  The two constraints do not conflict; the draft treated "low enough to forbid an upward
  edge" and "too low to permit a downward one" as the same kind of bound when they are
  opposite directions of the same inequality.

  Verified against `crates/cratestack-macros/Cargo.toml` (`[dependencies]`, `cargo metadata
  --no-deps`): `cratestack-macros`'s only normal workspace dependencies are `cratestack-core`,
  `cratestack-parser`, `cratestack-policy`, `cratestack-proto` — all L0/L1. **`L1` is a
  complete, exception-free assignment**: every dependent of `cratestack-macros`
  (`cratestack-pg`, `cratestack-api`, `cratestack-sqlite`, all L5) sees `1 ≤ 5`; every one of
  macros's own dependencies is ≤ 1; and — checked, not assumed — no L2 crate depends on
  `cratestack-macros` anywhere in the current graph, so "must not depend on L2" is satisfied
  as a consequence of the number, not by a bespoke predicate forbidding it. `cratestack-proto`
  depends only on `cratestack-core` (L0), so the identical argument places it at L1 too.
  Neither crate needs the `compiler` role this ADR originally proposed. `docs/adr/layers.toml`
  assigns both `cratestack-macros = 1` and `cratestack-proto = 1` on this basis, with the
  full argument repeated in that file's comments so it doesn't require re-deriving from this
  ADR.

  This does not retire the vocabulary in `layering.md`/ADR 0011 — "⊥, the compiler" remains
  the correct *prose* description of what `cratestack-macros` is (a thing that emits into L2
  through L5, not a layer those crates pass through). What it retires is the belief that the
  checker needs a second predicate to express it. If a future PR adds an edge from an L2
  crate into `cratestack-macros` or `cratestack-proto`, that edge fails this check exactly
  like any other upward edge — the plain `dep.layer ≤ self.layer` comparison already covers
  it at `L1`.
- **`cratestack-core` is a stated exception** (ADR 0011) — twenty modules spanning L0, L1
  and shared runtime vocabulary — so any single number for it is approximate by
  construction. It has zero workspace dependencies, so the approximation is harmless *for
  this check* and only for this check.

Five crates had no assignment in `layering.md` §2 at all — `cratestack-cbor-napi`,
`cratestack-cbor-wasm`, `cratestack-mock-wiremock`, `cratestack-proto` (placed only
"alongside ⊥"), and the empty vitrine `cratestack`. ADR 0011 now assigns all five; that
ADR's table, not §2's, is the checker's input.

So the question is not "check or not." It is: what is the table, and what does the check
do about the two real edges it finds today.

## Decision

**Settled.** CrateStack makes the layer assignment *data* and checks it in CI. A checked-in
table (`docs/adr/layers.toml`, kebab-case per repo convention) maps every `cratestack-*`
workspace crate under `crates/` to a layer number (`0`–`5`) or to one of two named
non-numeric roles: `tool`, and `vitrine` (the single empty `cratestack` crate — see that
file's header for why it isn't folded into `tool`). **`cratestack-macros` and
`cratestack-proto` get real layer numbers (`1`), not a role** — the "Amendment" note above
and the corrected Context bullet record why the originally-proposed `compiler` role turned
out to be unnecessary; the maintainer's instruction for this implementation was explicit that
the compiler crate gets a real layer number rather than an exemption, and the graph agrees.
A CI step (`.ci/layer-direction-check.sh`, wired as `just verify-layering` and the
`layer-direction` job in `.github/workflows/ci.yml`) reads `cargo metadata --no-deps
--format-version=1` and fails on any edge that violates the rule. Scope, fixed:

- **Normal dependencies only** (`kind == null`), **including target-gated ones.** Dev- and
  build-dependencies are exempt. Dev: established precedent — `cratestack-redis`
  legitimately dev-depends on `cratestack-axum` post-#465, for `Layer` types in its own
  tests. Build: decided here, on the same reasoning — a build-dependency compiles and runs
  only at the depending crate's own build time and never ships in a downstream consumer's
  resolved graph or the crate's own linked output. At the time of writing no `cratestack-*`
  crate has a build-dependency on another `cratestack-*` crate (`cratestack-studio`'s only
  build-dependencies are external — `flate2`, `tar`), so this choice changes no currently
  observable pass/fail outcome; it is decided now so a future build-dependency edge doesn't
  reopen the question under deadline pressure.
- **An unassigned crate fails the check.** Adding a crate forces a layer decision; it
  cannot be silently unclassified. Verified: deleting an entry from `docs/adr/layers.toml`
  and re-running the checker fails, naming the crate (see this PR's Verification section).
- **Tools get a role, not a number.** `layering.md`'s "tools may depend on anything and be
  depended on by nothing" is already self-contradictory at `origin/main` (`cli → studio`,
  `cli → migrate`, `cli → mock-wiremock`, `studio → migrate` are all tool→tool). The role
  predicate: a tool may depend on anything including other tools; nothing outside the
  tool/vitrine set may depend on a tool. This is the one non-numeric predicate the checker
  still needs — resolving the compiler question did not remove it, since tools genuinely
  have no single defensible layer number (a tool legitimately depends on facades, which are
  the highest numbered layer, while itself being depended on by nothing above L0–L5/⊥).
- **The two real `#475` edges are tracked, not fixed, in this PR.** `cratestack-client-store-
  {sqlite,redis} → cratestack-client-rust` stay at L2 in `docs/adr/layers.toml`, matching
  ADR 0011 §2's already-Accepted placement — this ADR does not reopen crate placement, only
  enforcement. The violation is recorded as two narrow, dated, per-edge entries in
  `.ci/layer-direction-allowlist.toml`, each citing #475, so CI is green while the defect
  stays visible and trackable rather than silently passing or silently blocking all
  unrelated PRs on an unrelated architectural decision. An allowlist entry that stops
  matching a real violation (edge fixed, or edge removed) is itself a hard CI failure — see
  that file's header — so the allowlist cannot quietly outlive the defect it documents.
  Repairing the edge (moving `ClientStateStore` down out of `cratestack-client-rust`, or
  reclassifying the client-side stores) remains out of scope here, per ADR 0011's Deferred
  section and this PR's own constraints.

**The three questions the original draft of this ADR left to the maintainer are resolved as
follows** (numbering matches the original list, preserved for anyone diffing against the
Proposed version): (1) **block**, not report — `layer-direction` is a required, non-optional
CI job, matching `feature-matrix`'s precedent; (2) the two real edges are **tracked via a
per-edge allowlist, not fixed and not laundered into a table change** — see the bullet
above; (3) **tools keep a role predicate; the compiler does not need one** — it gets a real
number instead, which is a strictly simpler outcome than either alternative the original
draft posed.

**What settled it:** the dataset the original Context table promised — one real defect class
(two edges, both now allowlisted against #475), the target-gated-section handling (verified
against `cratestack-sqlite → cratestack-client-rust`), and the tool/vitrine role predicate —
is the whole dataset once the ⊥ predicate is retired as unnecessary. No second exemption
clause was needed to make the check pass; the allowlist file is the one and only escape
hatch, and it is per-edge and self-expiring by construction (see above).

## Consequences

### Positive

- Catches the #465 class automatically. That class has now occurred twice (server side,
  fixed; client side, open), which is enough to call it a pattern rather than an accident.
- Forces a layer decision at crate-creation time, when it is cheap, instead of at the
  next audit.
- Makes `layering.md` executable rather than aspirational. A rule stated only in prose
  degrades into a habit — §1 of that document says so about CLAUDE.md's one-line version.
- Costs no compilation. `cargo metadata --no-deps` is already the first thing CI runs.

### Negative

- **Crate granularity is the wrong granularity for this workspace's worst case.**
  `cratestack-core` spans L0, L1 and runtime vocabulary in twenty modules. The check can
  never see a `schema → store` import inside it, because `cargo metadata` does not model
  modules. #465's defect happened to be crate-granular. The next one need not be.
- **The checker is not one comparison, but it is one fewer than originally estimated.**
  Retiring the ⊥ predicate (see the Amendment) removes one of the four branches the original
  draft counted; the tool/vitrine predicate and target-gated-section handling remain. The
  implementation does not currently check same-layer acyclicity at all (`docs/adr/layers.toml`
  assigns each L1 crate a plain integer and the script compares integers; it does not build a
  same-layer subgraph and run cycle detection on it) — the real graph has no same-layer cycle
  today (`cratestack-sql → cratestack-policy`, `cratestack-macros → {cratestack-policy,
  cratestack-proto}`, all one-directional), so the omission is currently unobservable, but it
  is a real gap against `layering.md` §3's full rule and is recorded as unresolved rather than
  silently scoped out.
- **The dev-dependency exemption is a real hole.** It is also load-bearing (see
  `cratestack-redis`), so it cannot simply be closed. A trait living in the wrong crate,
  with only tests proving it, is invisible to this check by design.
- **One more file that drifts, and it drifts permissively.** A mislabelled crate widens the
  check silently; nothing fails. `deny.toml` is the local precedent for how an allowlist
  file ages here — roughly sixty lines of per-crate justification prose, each entry
  individually defensible, the whole increasingly hard to audit.
- **It institutionalises the six-layer vocabulary before L3 exists.** If `OpExecutor` lands
  (`rpc-transport.md` §4, gated in §6.5, ADR 0015), every L4/L5 crate keeps its number but
  the meaning of the gap between L2 and L4 changes, and the table has to be re-argued
  rather than re-numbered.
- A blocking check makes an urgent crate addition a two-file PR.

### Deferred / revisit when

- **L3 lands.** Re-argue the table, not just extend it.
- **A second exemption clause is proposed** on top of the tool/vitrine predicate (the only
  remaining non-numeric role after this amendment). One clause is what shipped; a second is
  a sign the layer model is being bent to fit the graph rather than the reverse.
- **Same-layer acyclicity is added to the checker**, or a same-layer cycle is found that it
  would have caught. Whichever comes first should also close the gap noted under Negative
  above.
- **A direction violation is found that this check did not catch.** That would demonstrate
  crate granularity is insufficient and put module-level checking (or splitting
  `cratestack-core`) back on the table — both of which `layering.md` §7 deliberately leaves
  open, and neither of which this ADR decides.

## Alternatives considered

**Status quo — vigilance, with the table documented and nothing enforcing it.**
The strongest case is that it worked: #465 was found and fixed inside one release cycle by
a maintainer running `cargo tree -i`, and `layering.md` now writes the rule down where a
reviewer can cite it. For a pre-1.0, lockstep-versioned workspace with one primary
maintainer, a reviewer who knows the rule is cheaper *and more accurate* than a table that
must be maintained — a table has no judgement, and three of the five edges it flags today
need judgement. This is the closest alternative and the decision against it is not
comfortable. It loses on one fact: the `client-store-* → client-rust` edges have existed
since those crates were added, vigilance did not find them, and enumerating edges
mechanically did — on the first run.

**`cargo deny`'s `[bans]` section.** Already installed, already in `just all-checks`,
already in CI, no new tooling, no new file format. Rejected because `[bans]` denies crates
and versions globally, not per-source-crate direction. The nearest primitive,
`[[bans.deny]].wrappers`, inverts the question: it allowlists who may depend on a banned
crate. Expressing "no L2 crate may depend on any L4 crate" would need one entry per L4
crate, each enumerating every legal dependent by hand — the whole graph transcribed into a
file that must be edited on every crate addition, with no layer numbers anywhere in it.

**Derive the layers from the graph instead of checking against a table.** Zero maintenance,
nothing to drift: assign each crate its topological rank and require edges to decrease.
Rejected because it is vacuous. A direction violation is, by construction, consistent with
whatever rank the current graph produces — the graph is acyclic *including* the two bad
client-store edges. Checking a graph against itself detects only cycles, which `cargo`
already rejects. A layer is an intent; intent has to be written down somewhere the graph
cannot overwrite.

**Enforce at `rustc` time instead of in CI.** No external tool, fails where the developer
is. Rejected because Rust offers no lint for "this crate must not be in my dependency
graph" — the unit rustc sees is the `extern crate` edge, which is exactly what compiles
cleanly today. The one compile-time guarantee this repo does have,
`cratestack-api` structurally omitting `cratestack-sqlx` plus
`guard_server_postgres_backend`'s `compile_error!` (`no-database-mode.md` §7), works
*because* it is about a dependency's absence — a Cargo fact, not a rustc one. Which returns
to `cargo metadata`.

**Split `cratestack-core` so the table stops lying.** This would remove the check's largest
blind spot at its root rather than documenting around it. Rejected *here* because it is a
different decision: `layering.md` §7 leaves it deliberately open, it touches every crate in
the workspace, and folding it into an enforcement ADR would make this ADR decide two
things. It belongs in its own.

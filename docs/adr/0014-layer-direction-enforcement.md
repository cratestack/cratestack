# ADR 0014: Mechanical Enforcement of Downward-Only Dependencies

## Status

Proposed

## Date

2026-08-08

Context doc: [docs/design/layering.md](../design/layering.md)

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
- **⊥ has no number.** `cratestack-macros` and `cratestack-proto` are not a layer. L5
  depends on `cratestack-macros`; `cratestack-macros` must not depend on L2. Under a plain
  `layer(src) ≤ layer(dst)` comparison there is no integer that satisfies both — anything
  low enough to forbid `macros → sqlx` is too low for `pg → macros`. ⊥ needs a distinct
  predicate ("⊥ may depend on ≤ L1; anything may depend on ⊥"), which means the checker is
  not a one-line comparison even before same-layer acyclicity is added.
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

**Not settled. This ADR proposes; the maintainer decides.**

The proposal: CrateStack will make the layer assignment *data* and check it in CI. A
checked-in table (`docs/adr/layers.toml`, kebab-case per repo convention) maps every
`cratestack-*` workspace member to a layer number or to one of two named non-numeric roles
(`compiler`, `tool`). A CI step reads `cargo metadata --no-deps --format-version=1` and
fails on any edge that violates the rule. Scope, fixed:

- **Normal dependencies only** (`kind == null`), **including target-gated ones.** Dev- and
  build-dependencies are exempt — `cratestack-redis` legitimately dev-depends on
  `cratestack-axum` post-#465, for `Layer` types in its own tests.
- **An unassigned crate fails the check.** Adding a crate forces a layer decision; it
  cannot be silently unclassified.
- **Tools and the compiler get roles, not numbers.** `layering.md`'s "tools may depend on
  anything and be depended on by nothing" is already self-contradictory at `origin/main`
  (`cli → studio`, `cli → migrate`, `cli → mock-wiremock`, `studio → migrate` are all
  tool→tool). The role predicate is: a tool may depend on anything including other tools;
  nothing outside the tool set may depend on a tool. The compiler may depend on L0/L1 and
  nothing else; anything may depend on the compiler.

**What the maintainer must decide, explicitly:**

1. **Block or report.** Failing the `check` job, or a non-blocking annotation.
2. **The two real edges.** Either `ClientStateStore` moves down out of
   `cratestack-client-rust` — #465's move applied to the client half, a breaking
   public-path change this repo has shipped before (#453/#454, `32f89de`, the day before
   this was written) — or the table is corrected to place the client state stores *above*
   `cratestack-client-rust`, which means conceding that "adapter" is a role, not a layer,
   and that ADR 0011's L2 is two different things.
3. **The tool/compiler predicates.** Whether the added complexity of two non-numeric roles
   is worth it, or whether a cruder "tools and ⊥ are unchecked" exemption is honest enough.
   The second is simpler and gives up the ability to catch `macros → sqlx`, which is the
   single most damaging edge the workspace could grow.

**What would settle it:** the dataset in the Context table is the whole dataset — one real
defect class (two edges), three table/rule artifacts, three structural complications, at 30
workspace members. If that ratio holds after decision 2 is made, the check earns its keep.
If making it green needs a second exemption clause on top of the two role predicates, it
does not.

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
- **The checker is not one comparison.** Same-layer acyclicity, the ⊥ predicate, the tool
  predicate and target-gated sections each add a branch. "It's twenty lines of Python" is
  the honest floor, not the honest estimate.
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
- **A second exemption clause is proposed** on top of the tool and compiler predicates. One
  is a correction; two is a sign the layer model is being bent to fit the graph rather than
  the reverse.
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

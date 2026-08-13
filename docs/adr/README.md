# Architecture Decision Records

## Where ADRs live

The ADR series is **shared across two repositories**, split by audience:

- **`cratestack-docs/internals/*-adr.md`** — decisions that shape the *user-visible* surface
  (`.cstack` grammar, transport semantics, migration behaviour). Published to
  <https://cratestack.dev/internals/>, and referenced from this repo by URL.
- **`docs/adr/NNNN-slug.md`** (here) — decisions about the *internal shape* of the workspace:
  crate boundaries, dependency direction, where a concern is allowed to live. These have no
  user-facing surface and would be noise on the docs site.

Both halves draw from **one numbering series**, so a number identifies a decision unambiguously
regardless of which repo holds it.

## Numbering

| Range | Status |
|---|---|
| 0001–0005 | Written, in `cratestack-docs/internals/` |
| 0006–0010 | **Reserved** by ADR 0001's "Follow-Up ADRs" list — unwritten, do not reuse |
| 0011– | This repo's internal-shape series |

The gap is deliberate. ADR 0001 (`cratestack-docs/internals/core-architecture-adr.md`) names five
planned ADRs at 0006–0010 (COSE envelope modes, migration strategy, relation loading, privileged
operations, multi-framework support). None are written yet, but squatting those numbers would
either collide later or silently retire five planned decisions. Note the reservation list is
already stale in its *titles* — the written ADR 0003 is "SQL Views as Projections of Models",
while the list predicted "Permission Expression Semantics" — so treat it as a topic reservation,
not a promise about content.

**Before adding an ADR:** take the next free number across *both* repos, not just this directory.

## Index

| # | Title | Status |
|---|---|---|
| [0011](0011-architecture-layer-model.md) | Architecture layer model | Accepted |
| [0012](0012-no-ioc-container.md) | No IoC container | Accepted |
| [0013](0013-facade-disjointness-invariant.md) | Facade disjointness invariant | Accepted |
| [0014](0014-layer-direction-enforcement.md) | Layer direction enforcement | Accepted |
| [0015](0015-op-executor-l3.md) | OpExecutor as the L3 execution layer | Proposed |
| [0016](0016-store-spi-scope.md) | Store SPI scope | Proposed |

Context for 0011–0016: [docs/design/layering.md](../design/layering.md).

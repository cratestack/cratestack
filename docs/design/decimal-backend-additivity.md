# Decimal-backend feature additivity (cratestack#505)

Status: **implemented.** The maintainer chose Direction 2, the associated-type/marker shape (§7's
option (b)), and it has shipped — see §13 for what actually landed, how it differs in mechanism
(not outcome) from §7's own sketch, and the verification evidence. §§1–12 below are the original
spike/decision document, kept verbatim as the historical record of the evidence the decision was
based on — do not edit them to retroactively match the implementation; §13 is where the "what
actually happened" account lives.
Scope: `cratestack-core`'s `decimal-rust-decimal`/`decimal-bigdecimal` Cargo features and every
crate that references `cratestack_core::Decimal` or `cratestack_sql::SqlValue::Decimal`.
Tracking: cratestack#505. Direction 3 (drop the "neither selected" hard error) already **shipped**
in #521 + #525 — see §1. Direction 2 (make "both selected" additive instead of a hard error)
shipped in the PR this document's §13 describes.

## 1. What #521/#525 already fixed, and what is still open

Confirmed by reading `crates/cratestack-core/src/decimal.rs` (merged content) and `gh pr view 521`,
`gh pr view 525`:

- **#521** (merged) dropped the `compile_error!` that fired when *neither* `decimal-rust-decimal`
  nor `decimal-bigdecimal` was selected. `Decimal` (and everything that referenced it
  unconditionally — `cratestack_core::validators::validate_range_decimal`,
  `cratestack_sql::SqlValue::Decimal`, `IntoSqlValue for Decimal`, the sqlx/rusqlite bind/decode
  arms) is now `#[cfg]`-gated to simply not exist in that configuration, rather than hard-failing
  every backend-agnostic consumer.
- **#525** (merged, stacked on #521) removed a `resolver = "1"` workaround in
  `cratestack-macros/Cargo.toml` that existed only to paper over the same "neither" bug inside
  trybuild's synthetic target-context feature pool. No behavior change beyond removing CI noise.
- **What neither PR touched:** the *other* `compile_error!` two lines below the one #521 removed —
  the one that fires when **both** features are selected at once:

  ```rust
  // crates/cratestack-core/src/decimal.rs:67-70
  #[cfg(all(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
  compile_error!(
      "cratestack: `decimal-rust-decimal` and `decimal-bigdecimal` are mutually exclusive — enable exactly one"
  );
  ```

  This is the issue's actual defect: two independent dependents that each choose one backend,
  correctly, in isolation, still can't share a build. #521's own module doc says as much in the
  section literally titled *"This mutual exclusivity is a graph-wide invariant, not a per-crate one
  (cratestack#505)"* (`decimal.rs:18-32`) — it documents the problem prominently, it does not solve
  it, and says so.

So this document covers exactly the milder-half/real-defect split the task description draws:
the "neither" half is closed; the "both" half — non-additivity — is what remains, and is what
Directions 1/2/4 below are about.

## 2. Reproducing the residual defect

Executed, not inferred. Built as two tiny throwaway crates in a scratch directory outside this
repo (per the task's guidance), each depending on this worktree's `cratestack-pg` via a `path`
dependency with `default-features = false` and one backend selected — the issue's exact shape,
against the *post-#521/#525* state of this branch:

```toml
# crate-a/Cargo.toml
[dependencies]
cratestack = { package = "cratestack-pg", path = "…/crates/cratestack-pg", default-features = false, features = ["decimal-rust-decimal"] }

# crate-b/Cargo.toml
[dependencies]
cratestack = { package = "cratestack-pg", path = "…/crates/cratestack-pg", default-features = false, features = ["decimal-bigdecimal"] }
```

Each alone:

```
$ cargo check -p crate-a
    ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.73s

$ cargo check -p crate-b
    ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.39s
```

Both in one workspace (`cargo check --workspace` over the two-member scratch workspace — the
issue's own "together" step):

```
$ cargo check --workspace
    Checking cratestack-core v0.7.12 (…/crates/cratestack-core)
error: cratestack: `decimal-rust-decimal` and `decimal-bigdecimal` are mutually exclusive — enable exactly one
  --> …/crates/cratestack-core/src/decimal.rs:68:1
   |
68 | / compile_error!(
69 | |     "cratestack: `decimal-rust-decimal` and `decimal-bigdecimal` are mutually exclusive — enable exactly one"
70 | | );
   | |_^

error[E0432]: unresolved import `crate::Decimal`
  --> …/crates/cratestack-core/src/validators.rs:10:5
   |
10 | use crate::Decimal;
   |     ^^^^^^^^^^^^^^ no `Decimal` in the root
error: could not compile `cratestack-core` (lib) due to 2 previous errors
```

Confirmed by execution: the reported defect is real, current, and unchanged by #521/#525 — the
"both" `compile_error!` still fires and still cannot be worked around by either dependent alone.
(The scratch repro was against `cratestack-pg 0.7.12`, this branch's in-tree version, not the
issue's `0.7.10` from crates.io — the mechanism is identical; #521/#525 changed the "neither" arm
only, and this run proves the "both" arm is untouched.)

## 3. Exact `#[cfg]` guard inventory

Every unconditional reference to `cratestack_core::Decimal` was gated as part of #521. The full set
of guarded sites, confirmed by reading each file (all use the identical
`#[cfg(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]` predicate unless
noted):

| Crate | File | What's gated |
|---|---|---|
| `cratestack-core` | `src/decimal.rs:67-76` | The `compile_error!` (both-arm) and the two `pub type Decimal = …` aliases |
| `cratestack-core` | `src/lib.rs:56-57` | `pub use decimal::Decimal` re-export |
| `cratestack-core` | `src/lib.rs:90-94` | `pub use validators::validate_range_decimal` |
| `cratestack-core` | `src/validators.rs:10` | `use crate::Decimal` (unconditional import — the crash point in §2's repro) |
| `cratestack-sql` | `src/values/sql_value.rs:17-18` | `SqlValue::Decimal(cratestack_core::Decimal)` variant |
| `cratestack-sql` | `src/values/into_sql.rs:57-62` | `impl IntoSqlValue for cratestack_core::Decimal` |
| `cratestack-sqlx` | `src/query/support/values.rs:35-37,46-47` | `push_bind_value`'s `SqlValue::Decimal`/`NullDecimal` match arms (`unreachable!()` fallback when ungated, line 59-63) |
| `cratestack-rusqlite` | `src/value/columns.rs:41-51` | `DecimalColumn` newtype + its `FromSql` impl |
| `cratestack-rusqlite` | `src/value/bind.rs` | `SqlValue::Decimal` bind arm |
| `cratestack-rusqlite` | `src/value/decode.rs:29-34` | `decode_decimal` |

`SqlValue::NullDecimal` itself stays **unconditional** (it carries no `Decimal` payload — only
binding it needs the concrete type, per `sql_value.rs:13-16`'s own comment). `NullVector`/`Vector`
follow the identical two-tier gating pattern for `pgvector`, which is the precedent #521 copied.

This confirms the task's premise precisely: the "both" `compile_error!` is unmoved, and every
downstream site #521 gated is gated the *same* way (`any(...)`), so all of them fail together the
instant both features are unified into one build — exactly what §2 reproduced.

## 4. Blast radius — every place `Decimal` touches a public signature or trait impl

`grep -rn "Decimal" crates/ --include='*.rs'` returns 196 lines; the great majority are doc comments
or string-literal type-name matches in codegen (`"Decimal" => quote! { ... }`) that never name a
concrete Rust type. Filtered to actual public-API/trait-impl surface that a Direction-1/2 change
would have to touch:

**Directly typed on `cratestack_core::Decimal` (the hard cases — anything here pins one concrete type):**

1. `cratestack-core::decimal::Decimal` — the type alias itself (`decimal.rs:73,76`)
2. `cratestack-core::validators::validate_range_decimal(value: &Decimal, ...)` (`validators.rs:74`)
3. `cratestack-sql::SqlValue::Decimal(cratestack_core::Decimal)` (`sql_value.rs:18`) — the L1 shared
   enum every backend adapter matches on
4. `cratestack-sql::IntoSqlValue for cratestack_core::Decimal` (`into_sql.rs:58`)
5. `cratestack-sqlx::push_bind_value`'s `SqlValue::Decimal`/`NullDecimal` arms (`values.rs:37,47`) —
   the sqlx encode boundary, binds directly to `sqlx::QueryBuilder<Postgres>`
6. `cratestack-rusqlite::DecimalColumn(pub cratestack_core::Decimal)` + its `FromSql` impl
   (`columns.rs:43,46-51`) — the rusqlite decode boundary
7. `cratestack-rusqlite::decode_decimal(&str) -> FromSqlResult<cratestack_core::Decimal>`
   (`decode.rs:33`) and `format_decimal(&cratestack_core::Decimal) -> String` (`bind.rs:68`) — the
   rusqlite bind boundary (TEXT-affinity round trip)

**Codegen emission sites — these emit `::cratestack::Decimal` tokens into generated code, but never
construct or match a `Decimal` value themselves** (confirmed against `cratestack-macros/Cargo.toml`'s
own comment: "this crate's own proc-macro implementation never constructs a
`cratestack_core::Decimal` value, it only emits `::cratestack::Decimal` tokens"):

- `cratestack-macros::shared::sql.rs:124-128` — model field → `SqlValue` conversion
- `cratestack-macros::shared::types.rs:35,122-124` — field type → Rust type + parse-from-string
- `cratestack-macros::model::row_sqlite.rs:112-116` — row decode via `DecimalColumn`
- `cratestack-macros::procedure::types.rs:121,186` — procedure arg/return types
- `cratestack-macros::validators::emit.rs:79` — `@range` bound promotion
- `cratestack-macros::include::grpc_pb::scalar.rs:70,101-102` — gRPC wire (de)serialization

Every one of these is a `quote!`-time string match on `"Decimal"` (the *schema* type name, plain
text from `.cstack` source) that unconditionally emits the literal path `::cratestack::Decimal`.
None of them read a Cargo `cfg!` to decide *which* backend — they don't need to, because there is
only ever one `Decimal` path in scope today. This is the detail that makes Direction 2 expensive:
these sites would all need a second axis (which concrete type to name) that doesn't exist yet.

**Not in the blast radius, despite matching the grep:** `cratestack-client-dart`,
`cratestack-client-typescript`, `cratestack-proto`, `cratestack-migrate`, `cratestack-parser`,
`cratestack-studio`. These match `"Decimal"` as a schema type-name string (routing to a
Dart/TypeScript client type, a `NUMERIC` DDL keyword, a parser validation rule, a JSON-shaped studio
form field) and never reference `cratestack_core::Decimal` the Rust type at all — their behavior is
identical regardless of which Rust backend the server ends up compiled with. cratestack#498/#499
already gave the generated Dart/TypeScript clients their own independent real decimal types
(`package:decimal`, `decimal.js`) precisely so they don't need to agree with the server's backend
choice; that work is unrelated to this issue and unaffected by any of the directions below.

**Total: 7 hard sites across 4 crates** (`cratestack-core`, `cratestack-sql`, `cratestack-sqlx`,
`cratestack-rusqlite`) pin a concrete `Decimal` type; 6 codegen sites in `cratestack-macros` emit a
reference to whichever one is in scope without themselves choosing.

## 5. Why this is structurally hard, not just unfinished

Cargo builds **one** compiled artifact per (package, version) pair per build, with a feature set
that is the **union** of every requester's ask (absent unstable per-target feature unification,
which this workspace doesn't opt into). §2's repro shows this directly: `cratestack-core` is
compiled exactly once, and that one compilation sees both `decimal-rust-decimal` (from crate-a) and
`decimal-bigdecimal` (from crate-b) simultaneously — there is no way for the *same compiled
`cratestack-core` artifact* to be two different things for two different callers.

The same union applies one layer up, to `cratestack-macros` — and this repo has already run into
the general version of this problem once, documented in `docs/design/extensions.md` §2's "Enforcement
mechanism (revised after implementation)" note: the original design for extension-gating (#161)
assumed a proc-macro could read `CARGO_FEATURE_<NAME>` to see the *invoking* crate's features, and
`CARGO_FEATURE_<NAME>` turned out to be build-script-only — a proc-macro expands inside the
invoking crate's own `rustc` process but only ever observes **its own** (the proc-macro crate's)
compiled-in feature set, via `cfg!(feature = "...")`, which is itself subject to the same global
union. Concretely: if crate-a and crate-b in §2's repro both routed through the *same* generated
schema macro (they'd have to, since `cratestack-macros` is one shared dylib for the whole build),
`cfg!(feature = "decimal-rust-decimal")` *inside* `cratestack-macros` would see **both** features
active, the same as `cratestack-core` does today — the macro has no mechanism to ask "which backend
did *this particular* invoking crate choose," only "which backends does the whole build's unified
`cratestack-macros` have turned on." This is the load-bearing fact for §6 below: any fix that
routes the choice through a Cargo *feature* on a *shared* crate inherits this exact limitation,
regardless of whether the "both" `compile_error!` itself is removed.

## 6. Direction 1 — make the backends genuinely additive

Two structurally different ways to read "additive," with different costs:

### 6a. Precedence resolution (the issue's own first suggestion)

Remove the "both" `compile_error!`; when both features are active, `Decimal` resolves to one
backend by a documented rule (e.g. `rust_decimal` wins, matching today's `default`). Minimal diff —
delete `decimal.rs:67-70`, keep exactly one of the two `pub type Decimal = ...` arms unconditional
under `all(both)`.

**What this actually buys crate-b (§2's `decimal-bigdecimal` dependent):** nothing it asked for, if
crate-a (or *any* other crate anywhere in the same graph) also selects `decimal-rust-decimal` — which
is likely, since `decimal-rust-decimal` is `cratestack-core`'s `default`. The build now compiles, but
`Decimal` silently becomes `rust_decimal::Decimal` inside crate-b too, even though crate-b's
`Cargo.toml` explicitly asked for arbitrary precision. This is not hypothetical: `cratestack-core`'s
own README (`crates/cratestack-core/README.md:141-145`) documents that a value past
`rust_decimal`'s ~28-29 significant-digit cap that a `decimal-bigdecimal` build was relying on being
able to represent will now *silently* get the wrong type end-to-end — no compile error, no runtime
panic at the boundary that matters, just a `Decimal` that behaves differently than the crate's own
manifest says it should. That is a worse failure mode than today's loud `compile_error!`: today's
break is discovered at `cargo check` time, with a clear message; a silently-resolved precedence
would be discovered (if ever) as a production precision bug. **A precedence rule trades a compile-time
failure for a runtime correctness hazard**, which is the central reason this sub-option should not
be adopted as-is without a much louder signal than a Cargo feature choice at the point where a
consumer's actual precedence-loss risk becomes real (e.g. a `#[cfg(all(...))] fn(){}` deprecation
warning is not strong enough; nothing short of the current hard error reliably reaches a consumer who
never reads a changelog).

### 6b. Both types exposed under distinct paths, call sites choose

Expose `cratestack_core::RustDecimal` and `cratestack_core::BigDecimal` unconditionally (no feature
gate at all on the *type names* — only on which one a given piece of code, and its dependencies,
actually link), and let each schema/call site name the one it wants.

This does not survive contact with §4's inventory. `SqlValue::Decimal` (item 3) is a single L1 enum
variant shared by both database adapters and every generated model — it can hold **one** payload
type per compiled `cratestack-sql`, for the same union reason `cratestack-core` can currently hold
only one `Decimal` alias. Making `SqlValue` itself generic over the decimal type
(`SqlValue<D>`) is a real option, but it is Direction 2, not Direction 1 — see §7's discussion of
exactly this enum. Making `cratestack-core` unconditionally depend on **both** `rust_decimal` and
`bigdecimal`/`num-bigint` regardless of which one is "used" also directly violates an existing,
tested acceptance bar: `.ci/feature-matrix.sh` step `[6/7]`
(`assert_no_rust_decimal`) asserts `rust_decimal` is **not reachable at all** anywhere in the
resolved graph once `decimal-bigdecimal` is selected — precisely the guarantee cratestack#495 shipped
and that a `decimal-bigdecimal` consumer relies on to keep `rust_decimal` (and, transitively, its
own licensing/audit surface) out of their build. "Both types always compiled in" reopens that closed
gap to reopen this one.

So 6b is not actually a *third* option distinct from Direction 2 — it is Direction 2, described from
the "expose two paths" angle instead of the "generic parameter" angle. §7 covers its real cost.

### The sqlx/rusqlite encode-decode boundary

Both boundaries (§4 items 5-7) currently match on the *variant* (`SqlValue::Decimal`), not the
*type* — they already don't care which concrete `Decimal` they're holding, because there is only ever
one at compile time. Making the backend genuinely additive means these boundaries would need to pin
a concrete type **per bound value**, at the point a query is actually built — which is exactly where
6a's silent-precedence risk is sharpest: a Postgres `NUMERIC` column bound via `sqlx::QueryBuilder`
has one wire representation regardless of which Rust type produced it, so a precedence-resolved
`Decimal` writing through the sqlx boundary can silently write a value that has already lost
precision *before* it reaches the database, with no signal at either the compile or the query-execution
layer.

## 7. Direction 2 — move the choice off features entirely

A generic parameter or associated type on the traits that touch `Decimal`, so two dependents each
get the concrete type they asked for, resolved by monomorphization instead of a shared feature flag.

**What has to change, concretely, working from §4's inventory outward:**

- `SqlValue` (`cratestack-sql`, L1) becomes `SqlValue<D>` (or grows a `Decimal(Box<dyn DecimalLike>)`
  trait-object variant). Either way this is the single most expensive line in this whole document:
  `SqlValue` is matched exhaustively at ~20+ sites across `cratestack-sqlx` and `cratestack-rusqlite`
  (`read_source.rs`'s implementers, `find_many.rs`, `aggregate*.rs`, `support/conditions.rs`,
  `render/select.rs`, per `docs/design/layering.md` §4.3's own site count for the *twin* `ReadSource`
  trait). A generic parameter on `SqlValue` propagates into `ModelDescriptor<M, PK>`,
  `ReadSource<M, PK>`/`WriteSource<M, PK>` (`sql/src/descriptor/read_source.rs`, 12+14 required
  methods per `layering.md` §4.3), and therefore into every generated model struct's descriptor —
  becoming `ModelDescriptor<M, PK, D>` or forcing `D` to be resolved through an associated type on `M`
  itself.
- Every generated model struct (`cratestack-macros::model`) would need to either (a) become generic
  over a decimal type (`pub struct Order<D = DefaultDecimal> { total: D, ... }`) — which changes the
  signature of *every* generated model in the workspace, including ones with zero `Decimal` fields,
  purely because the shared descriptor machinery needs the parameter — or (b) pick the type via an
  associated type resolved by a marker (a `type Decimal = rust_decimal::Decimal;` item the schema
  owner writes once per crate, referenced by name in generated code). (b) is materially less invasive
  than (a) at the call-site level (existing model structs keep their current field types, just backed
  by whichever concrete type the crate-level marker names) but still requires threading that marker
  through every one of §4's 6 codegen emission sites, since they currently hardcode the bare path
  `::cratestack::Decimal` and would need to resolve it against the invoking crate's own marker
  instead — a real codegen change, not a signature-only one.
- The sqlx/rusqlite encode-decode boundary (§4 items 5-7) stops being a `match` arm and becomes a
  trait bound (`D: sqlx::Encode<'_, Postgres> + sqlx::Decode<'_, Postgres>` / a rusqlite `ToSql +
  FromSql` bound), which both backends already satisfy for `rust_decimal::Decimal` and
  `bigdecimal::BigDecimal` individually (per `decimal.rs`'s own doc: both implement `Clone`, `Debug`,
  `Display`, `FromStr`, `PartialEq`, `PartialOrd`, `Ord`, `Eq`, `Hash`, `Default`) — this part is the
  *cheapest* piece of the whole change.

**Ergonomics, stated honestly:** today a generated model struct's `Decimal`-typed field is just
`Decimal` — a plain, non-generic, `Copy`-or-not concrete type the app author never has to name or
parameterize. Under (a), every generic-parameterized model needs a type argument threaded through
every function signature that touches it (`fn find_by_id<D>(id: Pk) -> Order<D>` and so on,
recursively through relations — an `Order<D>` with a related `Account<D>` needs the *same* `D`, which
`cratestack-macros` would have to enforce, not the compiler for free), which is exactly the kind of
signature noise the framework has so far kept out of generated code (`cratestack-pg`/`cratestack-api`/
`cratestack-sqlite` are 246/156/75 lines total, per `layering.md` §2 — "a facade that grows a function
has stopped being a facade" is the same discipline this would strain). Under (b), the ergonomics cost
moves from every call site to a one-time per-crate marker declaration, which is a real, shippable
design (closer to how `Dialect` is kept to one method by design, per `layering.md` §5.6's quoted
rule — "new dialect-specific quirks should live in the backend's own renderer until at least two
backends agree on the shape"), but it is still a breaking public-API change to `SqlValue`,
`ModelDescriptor`, `ReadSource`/`WriteSource`, and every crate that names them directly (any code
outside codegen that constructs a `SqlValue::Decimal` by hand today) — none of that is free, and it
is the kind of change that should go through the same "maintainer decision" gate as the four
directions themselves, not be picked by this document.

## 8. Direction 4 — document the invariant, accept the limitation

**Already done, partially.** `crates/cratestack-core/src/decimal.rs:18-32` documents the
graph-wide-invariant framing prominently in the crate's own rustdoc, and `CLAUDE.md:130-152` restates
it for anyone reading this repo's own contributor docs. Both are genuinely good — but neither is
where an *external* consumer (the actual victim in cratestack#505 — a downstream `Cargo.toml` author
who has never opened this repository) encounters the choice.

**Where a consumer actually looks before hitting the `compile_error!`, and what's there today:**

- `crates/cratestack-core/README.md:27` (rendered on crates.io and docs.rs — the one place someone
  adds a dependency without cloning the repo) says: *"Exactly one `Decimal` backend feature must be
  selected... Selecting neither or both is a compile error."* This is now **stale on the "neither"
  half** — #521 explicitly made "neither" *not* an error — and it still does not mention that "both"
  can be forced on you by an unrelated dependency, which is the actual cratestack#505 scenario. A
  consumer reading this exact sentence today would believe they are protected as long as they
  personally pick one feature, which is precisely false in the two-independent-dependents case.
- `crates/cratestack-pg/README.md:58-76` documents the two features individually (what each backend
  is, the wire-format edge case past `rust_decimal`'s digit cap) — and repeats the identical stale
  claim: *"Mutually exclusive with `decimal-rust-decimal`; selecting neither or both is a compile
  error."* Same "neither" staleness as `cratestack-core`'s README, same missing cross-crate-forcing
  warning, in a second file a consumer is just as likely to read first (this is the facade README, the
  one actually named in a `Cargo.toml` dependency comment).
- Neither `cratestack-core`'s nor any facade's crate-level `//!` doc comment (`lib.rs`'s own module
  doc, which is what renders as docs.rs's front page — distinct from the `decimal` submodule's own
  doc, which a reader only reaches by clicking into it) mentions decimal backends at all.

If Direction 4 is the chosen path, the fix that actually matters is **not** more prose in
`decimal.rs` (already thorough) — it's promoting a short version of the same warning into the two
surfaces a consumer reads *before* adding the dependency: `cratestack-core/README.md`'s existing
(now-stale) sentence, corrected, and a one-line mention added to each facade's own README
"Installation"/"Features" section, cross-referencing this document. That is a small, low-risk,
purely-documentation change, orthogonal to whichever of Directions 1/2 (if either) the maintainer
later chooses — it should ship regardless, since it costs nothing and closes the "discoverable only
by hitting the error" gap the issue itself names as harm #4. (Flagged as a follow-up task rather than
fixed here, since editing README copy is still a repo change and this document's scope is the design
question, not the fix.)

**The honest cost of Direction 4 as a *permanent* answer, not just an interim one:** it does not fix
cratestack#505's headline scenario. Two independent library authors who each correctly read the
(corrected) documentation and each deliberately choose a different backend for good reasons — one
needs arbitrary precision, one wants to keep `rust_decimal` off their audit surface — still cannot
appear together in one binary, and the *fix* is not in either of their hands; it requires one of them
to change a decision the documentation told them was theirs to make. Direction 4 makes the failure
mode loud and early instead of silent, which is real, non-trivial value pre-1.0 — but it is
explicitly not what cratestack#505 asks for ("the actual fix" per the issue's own closing line is
Direction 1 or 2).

## 9. Does the facade-disjointness precedent transfer?

The four facades (`cratestack-pg`, `cratestack-api`, `cratestack-sqlite`, `cratestack-client`) solve
a superficially similar "consumers must pick one" problem via Cargo's `package =` rename, not via a
Cargo feature — `[lib] name = "cratestack"` in each facade's own `Cargo.toml`
(e.g. `crates/cratestack-pg/Cargo.toml:1-27`), with each facade published as a **separate package**
that happens to expose the same library name. `docs/design/layering.md` §4.1 calls this the strongest
part of the architecture, enforced by CI's `facade-disjointness` job.

**Why it works there and does not transfer here, mechanically:** Cargo's feature-union problem (§5)
only applies *within* one package identity — two dependents both needing `cratestack-core` at
different feature sets get unified onto one compiled artifact, but two dependents each depending on a
*different package* (`cratestack-pg` vs. `cratestack-sqlite`) are never unified at all, because
they're not the same crate. A top-level application can depend on a crate that pulls in
`cratestack-pg` (renamed `cratestack`) *and* a separate crate that pulls in `cratestack-sqlite`
(also renamed `cratestack`) in the same binary without conflict, because each is a fully independent
compiled rlib with its own private `extern crate cratestack` binding — nominal package distinctness
sidesteps unification entirely. That is *why* the facade split works as a "consumers must pick one"
mechanism: each facade is chosen once, by one **leaf** dependent (the application), and nothing
*else* in the graph needs to agree with that choice, because a facade is terminal — nothing depends
on `cratestack-pg` transitively expecting to also compose with a different facade's types.

`Decimal` fails that precondition on both counts. First, it is not a leaf choice — it's consumed by
shared, non-terminal infrastructure (`cratestack-sql::SqlValue`, `cratestack-core::validators`, the
sqlx/rusqlite encode-decode boundary) that itself has to be unified across the whole graph, the same
way `SqlValue` has to be one concrete enum for both database adapters to match on. Renaming the
*scalar type crate itself* (e.g. splitting a `cratestack-decimal-rust`/`cratestack-decimal-bigdecimal`
pair, each renamed `cratestack-decimal` at the consumer's discretion, exactly mirroring the facade
trick) would let two **leaf** consumers pick different scalar crates without conflict — but the moment
either of them needs to put that value into a `SqlValue` (i.e., ever touches a `model` field, which
is the entire point of the type), `SqlValue::Decimal`'s payload type is still fixed by whichever
`cratestack-sql` artifact is in the graph, and `cratestack-sql` is shared, non-renamed, non-leaf
infrastructure exactly like `cratestack-core` is today. The rename trick would relocate the union
collision from "which `Decimal` type alias exists in `cratestack-core`" to "which type
`SqlValue::Decimal` is allowed to hold in `cratestack-sql`" without eliminating it — it is not a
fifth direction, it is Direction 1/2 wearing the facade pattern's clothes, and it inherits Direction
2's real cost (§7) the instant it has to interoperate with `SqlValue`.

**The one place the pattern does transfer cleanly:** a consumer with **no** `Decimal`-typed model
field at all (the `provider = "none"`, no-`model` case #521 already fixed) never needs `SqlValue` to
hold a `Decimal` in the first place — which is exactly why #521's fix (gate `Decimal` out of existence
rather than force a choice) was the right shape for *that* half of the bug, and why it required no
facade-style crate split to land.

## 10. Recommendation

Ship the Direction-4 documentation fix now (§8 — cheap, no design risk, closes a real discoverability
gap and fixes a currently-stale README claim) regardless of what else happens. For the actual
non-additivity: this document does not pick between Direction 1 and Direction 2 on the maintainer's
behalf — that is explicitly the kind of call CLAUDE.md's AI-governance section reserves for the
human, and the issue itself frames both as legitimate, differently-priced options rather than asking
for a specific one. What this document does argue, from the evidence above: Direction 1's cheap
sub-option (6a, precedence resolution) trades a loud compile-time failure for a silent
runtime-precision hazard and should not ship without a much stronger, per-crate opt-in signal than a
Cargo feature default; Direction 1's other sub-option (6b) is not actually distinct from Direction 2
once `SqlValue` is accounted for; and the facade-rename precedent, while a genuinely good question to
ask, does not transfer past the leaf-vs-shared-infrastructure line `SqlValue` sits on. If forced to
rank: **Direction 2 with the associated-type/marker shape (§7's option (b))** is the only one of the
three that actually achieves what cratestack#505 asks for (two independent dependents, each getting
the backend they asked for, with no silent precision loss) without reopening the `rust_decimal`-leak
acceptance bar cratestack#495 already shipped — at the real, non-trivial cost of a breaking change to
`SqlValue`/`ModelDescriptor`/`ReadSource`/`WriteSource` and every one of §4's six codegen sites. Given
that cost, and that this is pre-1.0 with the limitation now loudly documented (post-#521, post
whatever ships from §8), **staying on Direction 4 a while longer is a defensible interim position**,
not a false economy — but it should be a stated, revisitable decision, not a silent default.

## 11. Rejected alternatives

- **Making `rust_decimal` an unconditional (non-optional) dependency of `cratestack-core`, with
  `Decimal` always resolving to it unless `decimal-bigdecimal` is explicitly selected.** This is
  `decimal.rs`'s own module doc's option (a), already considered and rejected there (`decimal.rs:47-52`):
  it would put `rust_decimal` back in the dependency tree of every `decimal-bigdecimal` consumer too,
  verified empirically against `cratestack-pg`'s `cargo tree` as cratestack#495's own acceptance bar.
  Rejected for the same reason here — it's a variant of §6a's precedence idea with the direction
  hardcoded, and inherits the same silent-precision-loss risk plus a permanent dependency-tree cost
  even for the backend that "loses."
- **A build-time environment variable / `CARGO_FEATURE_<NAME>`-style per-crate override.** Already
  empirically disproved for a materially similar problem in cratestack#161 (§5) — a proc-macro cannot
  observe the invoking crate's own feature set, only its own compiled-in one. Would not solve this
  issue even in principle.
- **Splitting the `Decimal` scalar into its own facade-style renamed crate pair, independent of
  `SqlValue`.** Considered in §9 and rejected as incomplete: it solves the problem for a consumer
  that never puts a `Decimal` value into a `model` field, which #521 already solved more cheaply by
  gating the type out of existence instead of splitting a crate. For any consumer that *does* declare
  a `Decimal` model field — the actual cratestack#505 scenario — `SqlValue`'s own shared, non-renamed
  identity reopens the identical union collision one layer up.

## 12. Verification notes

What was checked by execution vs. by reading, so the claims above can be trusted at the granularity
they're made:

- **Executed:** §2's three-command repro (`cargo check -p crate-a`, `-p crate-b`, and
  `--workspace` over a two-member scratch workspace outside this repo, `path`-depending on this
  branch's `crates/cratestack-pg`). Real terminal output, not reconstructed — the exact error text
  and line numbers in §2 are copy-pasted from the actual run.
- **Executed:** `gh pr view 521` and `gh pr view 525` to confirm both PRs are `MERGED` and to read
  their actual summaries, rather than trusting the task prompt's characterization of them.
- **Read, not executed:** every file/line citation in §3 and §4 (`decimal.rs`, `lib.rs`,
  `sql_value.rs`, `into_sql.rs`, `values.rs`, `columns.rs`, `bind.rs`, `decode.rs`, the
  `cratestack-macros` codegen sites) — confirmed by opening each file, not by grep alone (per this
  repo's own convention: grep narrows, read confirms). `grep -rn "Decimal" crates/ --include='*.rs'`
  (196 raw hits) was the starting point for §4; every hit was categorized by reading its surrounding
  context, not assumed from the matched line alone.
- **Read, not executed:** `.ci/feature-matrix.sh` in full (not just grepped) — its `[2/7]`,
  `[4/7]`, and `[6/7]` steps are cited in §1, §6b, and §11 respectively; the file's own comments
  (the "History" preamble) are the source for the #421/#495/#505 chronology in §1.
  **Not independently re-run** — CI's `feature-matrix` job already exercises it on this branch (per
  `.github/workflows/ci.yml:369-380`), and re-running it here would mean building most of the
  workspace's facade closures repeatedly, which conflicts with this task's "no `--workspace` build"
  constraint. The specific two assertions this document leans on (`[2/7]`'s "both backends is
  rejected" and `[6/7]`'s "no rust_decimal reachable under decimal-bigdecimal") are corroborated
  independently by §2's own repro (`decimal-rust-decimal`/`decimal-bigdecimal` together fails) and by
  reading the `Cargo.toml` feature-forward chains in §4/§9, respectively — not taken on the script's
  say-so alone.
- **Read, not executed:** `crates/cratestack-core/README.md`, `crates/cratestack-pg/README.md`,
  `CLAUDE.md`'s decimal section — confirmed the specific claims quoted in §8 (including the stale
  "neither... is a compile error" sentence) by reading the actual current file contents, not from
  memory of what such a README would typically say.
- **Not executed, and said so plainly:** no code for Direction 1 or Direction 2 was written or
  compiled — both are architectural proposals per this task's scope ("write no production Rust").
  The blast-radius and ergonomics claims in §6/§7 are reasoned from the actual current trait/struct
  shapes cited (`ReadSource`/`WriteSource`'s method counts, `SqlValue`'s match-site count) as recorded
  in `docs/design/layering.md` §4.3, cross-checked against this repo's own `grep -rn` output, not
  independently re-derived from scratch — flagged here so that count is understood as corroborated,
  not re-measured.

## 13. Implementation (closes §10's open question)

Direction 2, §7's option (b), shipped. This section records what actually landed, closing the
"staying on Direction 4 a while longer" interim position §10 left open — the graph-wide invariant
§5 describes no longer holds.

### 13.1 What changed, mapped to §4's inventory

- **`cratestack-core`** (`src/decimal.rs`): the "both selected" `compile_error!` is gone.
  `RustDecimal`/`BigDecimal` are now independently `#[cfg(feature = "decimal-rust-decimal")]` /
  `#[cfg(feature = "decimal-bigdecimal")]` re-exports (not `any(...)`/exactly-one gated) — both may
  exist in one compiled `cratestack-core` with no ambiguity, because nothing has to pick between
  them at this layer any more. The legacy `Decimal` alias is kept, still gated to *exactly one*
  feature (mirroring the "neither" treatment #521 already established: an ambiguous name simply
  doesn't exist rather than silently resolving one way, matching §6a's rejection of silent
  precedence). A new unconditional `DecimalValue` trait (blanket-implemented, structural — no
  dependency on either backend crate) is the bound every backend-agnostic call site is now written
  against.
- **`validators::validate_range_decimal`** is now `fn validate_range_decimal<D: DecimalValue>(...)`
  — generic, unconditional, no `#[cfg]` gate at all. This is a **cheaper outcome than §7 predicted**:
  §7 anticipated this site would need a marker/type-argument threaded in; instead making it generic
  removed the `#[cfg]` gate entirely, and every call site (`cratestack-macros`'
  `validators/emit.rs`) needed **zero changes** — `D` infers from the field's own concrete type at
  the call site. One of §4's six codegen sites turned out not to need touching; see §13.4.
- **`SqlValue::Decimal`** (`cratestack-sql`) now holds `Box<dyn DecimalLike>` — a new, unconditional,
  object-safe trait (`src/values/decimal_like.rs`) blanket-implemented for anything satisfying
  `DecimalValue`. This is the §7 "or grows a `Decimal(Box<dyn DecimalLike>)` trait-object variant"
  alternative explicitly named alongside `SqlValue<D>` — chosen over the generic-parameter route
  specifically because it does **not** propagate into `ModelDescriptor`/`ReadSource`/`WriteSource`
  or any generated model struct's own signature: none of those changed at all. This is materially
  cheaper than §7's own estimate, which treated `SqlValue` becoming generic (and that genericity
  necessarily propagating through the descriptor traits) as the load-bearing, most expensive part of
  the whole change.
- **The sqlx/rusqlite encode-decode boundary** (§4 items 5–7): rusqlite's side became fully generic
  with no downcasting needed (`DecimalColumn<D>`, `decode_decimal<D>`, `format_decimal(&dyn
  DecimalLike)` — TEXT round-trip only ever needs `Display`/`FromStr`, never the concrete type) —
  cheaper than §7 predicted. The sqlx side needed the downcast §7 anticipated:
  `cratestack-sqlx::push_bind_value` matches the `SqlValue::Decimal` variant, then downcasts the
  trait object to whichever concrete backend(s) that crate's own `decimal-*` Cargo features enabled
  (`Any::downcast_ref`), since `sqlx::QueryBuilder::push_bind` needs a concrete `Encode`-implementing
  type. At most one arm matches per actual value; both may be *compiled* at once.
- **Codegen** (`cratestack-macros`): the three entry macros (`include_server_schema!`,
  `include_embedded_schema!`, `include_client_schema!`) gained an optional trailing
  `decimal = RustDecimal | BigDecimal` argument, required exactly when the schema declares a
  `Decimal` field anywhere (`include/decimal_arg.rs::schema_uses_decimal` — models, mixins, custom
  types, views, and procedure args/return types, including through `Page<T>`/`FindMany<T>`); a
  schema with no `Decimal` field needs no argument, preserving cratestack#521's "neither" case
  unchanged. This is the schema-authored choice §7 called a "marker … the schema owner writes once
  per crate" — implemented as a macro argument rather than a hand-written `type` item, so there is
  no new manual-boilerplate contract for schema owners to maintain; the macro call site *is* the
  marker.

### 13.2 One deliberate mechanism deviation from §7's literal sketch, and why

§7 sketched the marker as something "referenced by name in generated code," which reads as a
parameter threaded explicitly through each of the six codegen call sites. That is not what shipped.
Instead, `crate::shared::decimal_backend` holds a `thread_local!`-scoped "which backend is active"
cell, set once per macro invocation (`with_decimal_backend`, wrapping the entire body of each entry
macro's composer) and read by the six sites via `current_decimal_type_tokens()` — an ambient/scoped
context rather than an explicit function parameter.

This is a deliberate implementation choice, not a scope cut, made for a concrete reason: `rust_type_
tokens`/`sql_value_tokens`/and friends have on the order of 30 call sites across the crate (primary-key
types, enum types, relation types, …), the overwhelming majority of which never touch `Decimal` at
all. Threading an explicit parameter through every one of them — mostly unused — is exactly the
signature-noise cost §7 itself warned Direction 2(a) would inflict on *generated* code; doing the
analogous thing to `cratestack-macros`' own *internal* call graph would reintroduce a smaller version
of the same cost one layer up, for no externally-visible benefit (the six sites' behavior is
identical either way: resolve the schema's chosen concrete type, per invocation, never via a Cargo
feature). The externally-visible contract §7 describes — model struct fields keep concrete,
non-generic types; the schema owner names the backend once; no `cfg!`-against-the-macro's-own-
feature-set anywhere — is unchanged. `cratestack-macros/src/shared/decimal_backend.rs`'s own module
doc states this reasoning inline; flagged here too since it's a legitimate place to disagree with
§7's literal wording, and the maintainer may prefer explicit parameter-threading instead — this was
not re-litigated, just implemented the cheaper way and disclosed.

### 13.3 Verification evidence

- **The decisive test (cratestack#505's own repro, executed before and after):** two throwaway crates
  in a scratch workspace, `crate-a` depending on this branch's `cratestack-pg` with
  `default-features = false, features = ["decimal-rust-decimal"]`, `crate-b` the same with
  `decimal-bigdecimal`. Against `origin/main` (`b0c559975e8`): `cargo check --workspace` over the
  two-member scratch workspace fails with the exact `compile_error!` + `E0432` pair §2 documented.
  Against this branch: the identical two-crate shape (functions returning `RustDecimal`/
  `BigDecimal` respectively) — `cargo check -p crate-a`, `-p crate-b`, and `--workspace` all succeed.
- **`.ci/feature-matrix.sh`**: step `[2/7]` inverted from "both selected is rejected" to "both
  selected compiles and both backends are independently usable" (with reasoning inline in the
  script), including a `cargo test` assertion that `both_decimal_backends_tests` actually runs. Every
  other step (facade default/narrowed matrices, the `[1/7]`/"neither" guarantee, the `[6/7]`
  `assert_no_rust_decimal` bar) is unchanged and still passes — run in full via `bash
  .ci/feature-matrix.sh`.
- **`cargo tree`, not reading code**, for the #495 acceptance bar: `cargo tree -p cratestack-pg
  --no-default-features --features postgres,decimal-bigdecimal -e features` and the `cratestack-client`
  equivalent (the same commands `.ci/feature-matrix.sh`'s `assert_no_rust_decimal` runs) confirm
  `rust_decimal` is still unreachable when only `decimal-bigdecimal` is selected — unchanged by this
  PR, verified by execution.
- **Real round-trips against a live Postgres** (`CRATESTACK_USE_TESTCONTAINERS=1
  CRATESTACK_REQUIRE_DB=1`, not skipped): `cratestack-pg`'s `banking_decimal`, `banking_validation`,
  `round_trip_types`, and — under `--no-default-features --features postgres,decimal-bigdecimal` —
  `decimal_bigdecimal_backend` (including its 40-significant-digit value, past `RustDecimal`'s
  capacity) all pass, proving the `bind_decimal` downcast at the sqlx boundary actually binds the
  right concrete type end-to-end, not just that it type-checks.
- **Embedded round-trips**: `cratestack-sqlite`'s `round_trip_types` and `sqlite_e2e` (real
  in-memory SQLite, not mocked) both pass, exercising the generic `DecimalColumn<D>`/`decode_decimal`
  rusqlite boundary.
- Every crate this PR touches (`cratestack-core`, `-sql`, `-sqlx`, `-rusqlite`, `-macros`, `-pg`,
  `-sqlite`) passes `cargo test` under default features, `--no-default-features` (neither backend),
  `--features decimal-bigdecimal` alone, and — where meaningful — both backends together.
  `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` (this workspace's
  `clippy_allow` set) are clean on all of them.

### 13.4 Corrections to §4/§7's own predictions, now that this is built

Flagged explicitly per this task's instruction to report when reality diverges from the design
doc's estimate — none of these made the change *harder* than §7 priced it; all three made it
*cheaper*, which is worth recording so a future reader trusts the estimate-vs-actual gap in the
direction it actually went:

1. §4 counted `validators/emit.rs` as one of six codegen sites needing a change. It needed none —
   making `validate_range_decimal` generic let type inference do the work.
2. §7's cost discussion centered on `SqlValue` becoming generic and that genericity necessarily
   reaching `ModelDescriptor`/`ReadSource`/`WriteSource`. The trait-object variant § 7 also named
   avoided that reach entirely — those three types, and every generated model struct, are
   byte-for-byte unchanged by this PR.
3. The rusqlite encode/decode boundary needed no downcasting at all (unlike the sqlx boundary,
   which does) — TEXT-affinity round-tripping only ever needs `Display`/`FromStr`, so it went fully
   generic instead.

No direction turned out to be *worse* than §7's analysis; the associated-type/marker shape held up
as the right call.

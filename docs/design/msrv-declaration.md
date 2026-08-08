## Design proposal for #422: MSRV declaration + CI toolchain consumption (revised)

> **Status: proposal, not a decision.** This document exists so the maintainer can make the judgement calls listed under "Decisions needed"; it is not an approved design. Nothing here is implemented.

This issue bundles two independent problems: (1) declaring/enforcing an MSRV, and (2) refreshing and wiring the `examples/no-database-verification*` verification workspaces into CI. Only (1) is blocked on a maintainer decision — (2) is mechanical and can ship as its own PR without waiting on this. This proposal covers only the blocked half, and now explicitly names that split as a decision rather than assuming it.

### Decisions needed (up front)

1. **What MSRV value to declare** in `[workspace.package].rust-version`.
2. **Whether to rewrite CI's 20 `dtolnay/rust-toolchain@stable` steps** (not 18 — corrected count below) to explicitly pin the toolchain, or keep them and add a dedicated, explicit MSRV-verification job instead.
3. **Whether this issue closes as one PR or two.** The five ACs split cleanly into an MSRV half (items 1, 2, 5) and a lockfile/CI-wiring half (items 3, 4). This proposal recommends shipping them as two PRs, but that is itself a judgment call the original proposal made silently — flagging it here so the maintainer can veto the split if a single PR is preferred.

### Evidence (verified against HEAD, 32f89de — re-verified independently, not taken on faith from the original proposal)

- `Cargo.toml:84-96` — `[workspace.package]` has `version = "0.7.6"`, `edition`, `license`, etc. — confirmed no `rust-version` key.
- `rust-toolchain.toml:6` — `channel = "1.95.0"`, pinned per its own comment "so `cargo fmt --check` and `cargo clippy -D warnings` are deterministic" — not currently propagated as `rust-version` anywhere; confirmed via `grep -rln rust-version crates/*/Cargo.toml` returning nothing.
- **Corrected count:** `dtolnay/rust-toolchain@stable` appears **20 times**, not 18: 13 in `ci.yml` (lines 25, 57, 85, 107, 147, 172, 200, 215, 261, 303, 419, 462, 524), 1 in `rustdoc-pages.yml:25`, 1 in `prepare-release.yml:71`, 5 in `release-cli.yml:93,127,166,401,575`. `grep -c` against each file confirms 13+1+1+5=20. The original proposal's own evidence list enumerated exactly these 20 line references but then summarized them as "18 total" in two places (Evidence section and the Axis-2 CI-consumption framing) — an internal arithmetic error that should not survive into the implementation PR, since Option 3's diff size and Option 4's "smallest diff" framing both depend on getting this number right.
- `.github/workflows/ci.yml:189-192` — existing maintainer comment: "Determinism comes from rust-toolchain.toml pinning the version (and clippy/rustfmt components)..." — the codebase already *believes* the toolchain file governs, but the original proposal treated this only as an assertion to be tested by a new CI job. **Independently verified the actual mechanism** by fetching `dtolnay/rust-toolchain`'s `action.yml`: the action runs `rustup toolchain install <toolchain> ...` then `rustup default <toolchain>` (the latter step wrapped in `continue-on-error: true`). It does **not** set `RUSTUP_TOOLCHAIN` and does **not** call `rustup override set`. Rustup's documented override precedence is: `RUSTUP_TOOLCHAIN` env > `+toolchain` CLI flag > directory override (`rustup override set`) > nearest-ancestor `rust-toolchain(.toml)` file > `rustup default` (global default). Since `dtolnay/rust-toolchain@stable` only ever touches the lowest tier (`rustup default`), and this repo's `rust-toolchain.toml` sits at the workspace root above every job's working directory, **the toolchain file already wins over @stable today, in all 20 occurrences, independent of any new CI job.** This is stronger grounding for Option 4 than the original proposal offered (which leaned on the maintainer's comment as authority rather than the action's actual mechanics) — but it also means the value of the new assertion step is confirmatory/regression-guarding, not first-discovery, and the proposal text should say so rather than implying the fact is currently unknown.
- `examples/no-database-verification/Cargo.lock:457-458` and `examples/no-database-verification-api/Cargo.lock:270-271` — confirmed locked `cratestack-pg = "0.6.3"` and `cratestack-api = "0.6.4"` against workspace `0.7.6` — still stale, unrelated to this half of the issue but cited as the drift precedent this proposal repeatedly leans on.
- `docs/design/` — confirmed no MSRV/toolchain-version-matrix precedent in any of its 11 files (only `protobuf.md` mentions "toolchain", referring to the protobuf compiler, not rustc).
- `crates/cratestack-pg/Cargo.toml:1-13` — confirmed the `X.workspace = true` inheritance pattern (edition, license, authors, repository, homepage, documentation, keywords, categories) that `rust-version.workspace = true` would extend.
- `justfile:406-445` (`bump` recipe) — confirmed it rewrites every `Cargo.toml` version literal atomically from one input, and has no step touching `rust-toolchain.toml` or the two example lockfiles — the single-source-of-truth pattern this proposal's Option 1 only partially follows (see below).
- `.github/workflows/ci.yml:19-45` (`check` job) — confirmed structure: `actions/checkout` → `tauri-linux-deps` → `dtolnay/rust-toolchain@stable` → `cargo metadata --locked` assertion → `Swatinem/rust-cache` → install `just` → `just check --locked`. The new `msrv` job should follow this same shape and reuse `just check --locked` rather than hand-duplicating the raw `cargo check --workspace --exclude embedded_flutter_native --all-targets` line, per the repo's existing convention of centralizing that command in the justfile.

### Options

| # | Axis | Option | Breaking (pre-1.0 lockstep)? |
|---|------|--------|---|
| 1 | MSRV value | `rust-version = "1.95.0"` (= the existing toolchain pin) | No new public API surface — but see caveat below |
| 2 | MSRV value | Empirically-determined lower bound via `cargo-msrv` | No |
| 3 | CI consumption | Rewrite all 20 `@stable` steps to `@1.95.0` | No |
| 4 | CI consumption | Keep `@stable`; add one explicit `msrv` job + a three-way version assertion | No |

**Breaking-change caveat:** "No" in the table above is true in the strict SemVer/public-API sense — pre-1.0, lockstep-versioned crates gain a new `Cargo.toml` field and (possibly) new CI jobs, no function signatures or types change. But declaring `rust-version = "1.95.0"` (a recent release) is a practical build break for any downstream consumer on an older toolchain — this is not hypothetical for a framework whose entire product is compiled/generated code. The issue's own "Risks" section already says "Declaring an MSRV is a support commitment"; the options table should not let "Breaking? No" read as "consequence-free." Recommend the PR description carry this caveat explicitly rather than relying on the table's terse "No."

**Axis 1 detail — MSRV value**

- *Option 1 (pin the toolchain version as MSRV):* Verified — via the rustup precedence mechanism above, not just the ci.yml comment — that it's the only Rust version any test, lint, or build in this repo has ever actually run under. The AC "CI job verifying the MSRV builds" is nearly free once one job says so explicitly. Downside, unchanged from the original: conflates "tooling we standardize CI on for determinism" with "oldest compiler we support" — every future deliberate toolchain bump becomes a public MSRV bump too. **Additional downside the original proposal underweighted:** "kept equal to rust-toolchain.toml's channel by convention" is an *unenforced* hand-sync claim — precisely the drift pattern the issue exists to fix (see the two stale example lockfiles). This needs the three-way CI assertion in Option 4 to actually hold, not just a stated intention.
- *Option 2 (empirical lower bound):* The textbook-correct MSRV, but confirmed zero supporting infrastructure exists in this repo today — no `cargo-msrv` config, no version matrix, nothing in `docs/design/`. Building that now imports a second instance of exactly the kind of maintenance-drift problem this issue is already about (see: the two stale `Cargo.lock` files at `examples/no-database-verification/Cargo.lock:1` and `examples/no-database-verification-api/Cargo.lock:1`, both stale against the workspace's `0.7.6`) — an untested lower bound is a claim nobody is checking on every PR.

**Axis 2 detail — CI consumption**

- *Option 3 (explicit pin everywhere, 20 occurrences):* Makes the pin visible in every job. Downside: 20 hardcoded occurrences of "1.95.0" become a second source of truth that has to be hand-kept in sync with `rust-toolchain.toml` — the opposite of this repo's own `justfile` `bump`-recipe convention. Given the verified rustup-precedence mechanism, this rewrite would very likely be behaviorally a no-op — its only value is defense against a future action-behavior change, which the assertion step in Option 4 already guards against more cheaply.
- *Option 4 (keep `@stable`, add one explicit job + three-way assertion):* Smallest diff. Directly answers the issue's AC "confirm whether CI's `@stable` toolchain actually resolves to the pinned version" with logged evidence. The assertion should check three quantities for equality, not two: resolved `rustc --version`, `rust-toolchain.toml`'s `channel`, and `Cargo.toml`'s `rust-version` — the original two-way (rustc vs. toolchain-file only) version leaves the rust-version-vs-toolchain-file sync path completely unchecked, which is exactly the gap Option 1's "by convention" con describes. Risk: if the assertion ever fails, a follow-up PR is needed — strictly better than rewriting 20 jobs first on an assumption that turns out to be a no-op.

### Recommendation

**Option 1 + Option 4, corrected.** Declare `rust-version = "1.95.0"` in `[workspace.package]`, matching `rust-toolchain.toml`'s existing pin. Leave the 20 existing `@stable` steps alone; add one new explicit `msrv` job pinned to `1.95.0` (reusing `just check --locked` and `./.github/actions/tauri-linux-deps`, per the existing `check` job's shape) plus a three-way `rustc --version` / `rust-toolchain.toml` / `Cargo.toml rust-version` assertion in the existing `check` job. If that assertion ever shows the rustup override isn't firing, escalate to Option 3 for the affected jobs as a narrowly-scoped follow-up — don't do the 20-way rewrite speculatively, and don't do it on the wrong occurrence count either.

This keeps the MSRV number honest, avoids inventing new unmaintained infrastructure, resolves the CI question with a verified mechanism rather than an appeal to a comment, and closes the specific drift loophole (rust-version silently diverging from rust-toolchain.toml) that the original two-way assertion left open.

### Implementation sketch (once the decision lands)

1. **`Cargo.toml`** — in `[workspace.package]` (after `edition = "2024"`), add:
   ```toml
   rust-version = "1.95.0"
   ```
   Each published crate picks this up via `rust-version.workspace = true`, following the existing `edition.workspace = true` / `license.workspace = true` pattern already in `crates/*/Cargo.toml` (verified in `crates/cratestack-pg/Cargo.toml`).

2. **`.github/workflows/ci.yml`** — add a new job, reusing the justfile recipe instead of duplicating the raw command:
   ```yaml
   msrv:
     name: msrv (1.95.0)
     runs-on: ubuntu-latest
     steps:
       - uses: actions/checkout@v6
       - uses: ./.github/actions/tauri-linux-deps
       - uses: dtolnay/rust-toolchain@1.95.0
       - uses: Swatinem/rust-cache@v2
       - uses: taiki-e/install-action@v2
         with:
           tool: just
       - run: just check --locked
   ```
   and add a three-way assertion step inside the existing `check` job:
   ```yaml
   - name: Assert rustc, rust-toolchain.toml, and Cargo.toml rust-version agree
     run: |
       resolved=$(rustc --version | awk '{print $2}')
       pinned=$(awk -F'"' '/^channel/{print $2}' rust-toolchain.toml)
       declared=$(awk -F'"' '/^rust-version/{print $2; exit}' Cargo.toml)
       echo "resolved rustc: $resolved | rust-toolchain.toml: $pinned | Cargo.toml rust-version: $declared"
       [ "$resolved" = "$pinned" ] || { echo "::error::CI resolved rustc $resolved, expected rust-toolchain.toml's $pinned"; exit 1; }
       [ "$pinned" = "$declared" ] || { echo "::error::rust-toolchain.toml ($pinned) and Cargo.toml rust-version ($declared) have drifted apart"; exit 1; }
   ```

3. **Keep the other 19 `@stable` occurrences unchanged** pending the assertion's result (19, not 17 — corrected for the 20-total count).

4. **Non-blocked, separable half of #422** (ship independently, no maintainer decision needed): refresh `examples/no-database-verification/Cargo.lock` and `-api/Cargo.lock` (currently pinned to `cratestack-pg 0.6.3` / `cratestack-api 0.6.4` against workspace `0.7.6`), add a `just bump` step to run `cargo update` in both directories, and add a CI job running `cargo tree --locked -i libsqlite3-sys` in each to keep the facade-disjointness guarantee enforced.

### Test strategy

- `cargo metadata --no-deps --format-version=1 | jq '.packages[] | select(.name=="cratestack-pg") | .rust_version'` (and the other published facades) to confirm `rust-version` is actually emitted in the resolved manifest — **corrected from the original's `cargo publish --dry-run`**, which mainly verifies the package builds and packages successfully under the *current* rustc; it does not itself surface or assert the `rust-version` field's value, so it's the wrong tool for this specific check. `cargo publish --dry-run` is still worth running once as a smoke test that publishing isn't otherwise broken, but shouldn't be cited as the verification for this AC.
- New `msrv` CI job going green is direct verification that `1.95.0` builds the full workspace (mirrors the existing `check` job's command via `just check --locked`, so no new failure surface).
- The three-way assertion step going green in a real CI run is the evidence requested by the issue's AC #5, and additionally guards against the rust-version/rust-toolchain.toml drift path — attach that log to the PR.
- No code-level unit tests are needed for this half of the issue; it is entirely manifest + workflow configuration, verified by CI itself passing under the new/changed jobs.

### Note on the issue body

The issue body (`gh issue view 422`) is well-formed data with no embedded directives to this agent — it correctly marks "Choosing the MSRV value itself" as out of scope for the fixing PR and asks for a maintainer decision, which is exactly what this proposal solicits. No instruction-injection concerns found in its text.

## Reviewer notes

What I checked and changed, and why:

1. **Fixed a real arithmetic error**: the original proposal's Evidence section lists all 20 line-number occurrences of `dtolnay/rust-toolchain@stable` (13 + 1 + 1 + 5) but then summarizes them as "18 total" in two places (Evidence bullet, Axis-2 framing). Re-verified by `grep -c` against all four workflow files: the true count is 20. This isn't cosmetic — Option 3's "rewrite all N steps" diff size and Option 4's "smallest diff" contrast both depend on the number being right, and "18" undercounts the very thing the proposal is arguing against rewriting.
2. **Strengthened, not weakened, the central technical claim for Option 4**: the original proposal treated "does rustup's toolchain-file override actually govern over @stable" as an open question to be settled by a new CI job, leaning only on a maintainer's in-repo comment as evidence. I independently fetched `dtolnay/rust-toolchain`'s actual `action.yml` and confirmed the mechanism: it runs `rustup default <toolchain>`, rustup's *lowest*-priority override, and never touches `RUSTUP_TOOLCHAIN` or a directory override — both of which would rank above the repo's `rust-toolchain.toml` file. This means the file already wins today, verifiable from public documentation without needing new CI infrastructure at all; the new job's role is confirmatory and regression-guarding, not first-discovery. This makes the recommendation *more* defensible than the original text argued, not less — but the original phrasing ("the maintainer's own comment... already asserts") was an appeal to authority where a mechanism-level citation was available and stronger.
3. **Closed a real gap in the assertion's own scope**: the original two-way assertion (`rustc --version` vs. `rust-toolchain.toml`) does nothing to check `rust-version` in `Cargo.toml` against either of those — meaning Option 1's own admitted weakness ("kept equal... by convention") stays completely unenforced even after this PR ships. Extended the assertion to three-way equality, which directly serves the issue's underlying purpose: preventing exactly this class of silent drift (the two stale example lockfiles are the issue's own proof that "by convention" fails in this repo over time).
4. **Added a third explicit decision**: the original proposal unilaterally decided to split this issue into two PRs ("Only (1) is blocked... (2) is mechanical and can ship as its own PR without waiting on this") without listing that split as something the maintainer should bless. It's a reasonable call, but it changes what "closing #422" means, and the maintainer might legitimately want one PR that satisfies all 5 ACs together. Added it to "Decisions needed."
5. **Added a breaking-change caveat**: the options table's blanket "Breaking? No" is correct in the strict SemVer sense but risks reading as "no consequences." Declaring a recent MSRV is a genuine practical build break for downstream consumers on older toolchains — the issue's own Risks section already says as much. Added an explicit caveat rather than let the table imply the opposite.
6. **Fixed the test-strategy's imprecise verification command**: `cargo publish --dry-run` doesn't surface whether `rust-version` was emitted correctly in the manifest — it just confirms packaging/build succeeds under the *current* toolchain. Swapped in `cargo metadata --format-version=1 | jq` against the `rust_version` field, which is the thing that actually needs checking.
7. **Minor convention fix**: had the new `msrv` job call `just check --locked` instead of duplicating the raw `cargo check --workspace --exclude embedded_flutter_native --all-targets --locked` line, matching how the existing `check` job already centralizes that command in the justfile.
8. **What held up unchanged**: both MSRV-value options and both CI-consumption options are real, substantively argued alternatives — none reads as a strawman built to be knocked down. The "no docs/design/ precedent, no cargo-msrv infra, lockstep-versioned workspace" reasoning against Option 2 is independently verified and correct. The recommendation itself (Option 1 + Option 4) survives review — it was just under-evidenced and had one uncovered drift path, both fixed above.
## Design proposal: cratestack#426 — grpc/service.rs arm builders + Dart/TS gRPC descriptor duplication (revised)

> **Status: proposal, not a decision.** This document exists so the maintainer can make the judgement calls listed under "Decisions needed"; it is not an approved design. Nothing here is implemented.

This defect is confirmed real at HEAD (585-line `service.rs`, five near-identical `build_*_arm` fns at lines 268-585, three unexplained `let _ = pk;` at 280/333/452; 130-136-line-per-file duplication between `crates/cratestack-client-dart/src/grpc/*` and `crates/cratestack-client-typescript/src/grpc/*`, both re-verified directly against the files in this pass). It's blocked on three judgement calls only the maintainer can make. This revision keeps the original recommendation on both structural decisions but corrects a precedent citation, a factually wrong sizing plan that would leave Acceptance Criterion #1 unmet, and a golden-test description that doesn't match what its own cited precedent does.

### Decisions needed

1. **service.rs shape** — plain `ArmSpec` config struct + one `build_unary_arm()`, or a trait with a per-verb impl?
2. **service.rs sizing** — does hitting AC1 (~200-LoC ceiling) require splitting `build_service`'s `ApiServer`/`into_router` scaffold into a second file, in the *same* PR as the `ArmSpec` extraction (not as a contingency)?
3. **Dart/TS descriptor home** — new small shared crate, or a new module inside `cratestack-proto`?
4. **Golden-file approach** — confirm reuse of two patterns already in this repo, correctly described, before either refactor.

### Decision 1 — service.rs arm builders

| Option | Breaking | Pros | Cons |
|---|---|---|---|
| **A. `ArmSpec` config struct + `build_unary_arm()`** (recommended) | No | Matches `ModelHandlerPrep` (`crates/cratestack-macros/src/axum/model/prep.rs`) — a per-model struct with ~15 `proc_macro2::TokenStream` fields (capabilities, preflight checks, etag handling) consumed by "the handler/builder emitters," verified in the same crate and structurally identical to what `ArmSpec` proposes. This is a materially closer precedent than `ModelDescriptor` (which is a *runtime-emitted metadata const*, not a macro-internal builder-config) or `TypeScriptGeneratorConfig` (a public generator config with no `TokenStream` fields at all) — both cited in the original pass but neither is actually the same pattern. Confirmed zero `trait` definitions anywhere in `cratestack-macros/src` (grep), so "no precedent for trait dispatch" is literal fact, not framing. Fixes the `let _ = pk;` criterion for free — confirmed genuinely dead: `build_list_arm`/`build_create_arm` already take 2 args (no `pk`) while `build_get/delete/update_arm` take 3, so the five fns were never uniform to begin with; nothing needs a matching signature since call sites are direct (`service.rs:86-92`, confirmed) | The "decode" fragment needs a corrected definition (see below); and this refactor *alone*, without the sizing fix in Decision 2, cannot satisfy AC1 |
| **B. `trait ArmKind` + `build_unary_arm<K: ArmKind>()`** | No | Slightly more "pluggable" if a 6th verb appears; groups per-verb logic behind one trait | No runtime dispatch need at macro-expansion time, so generics buy nothing over a struct; zero precedent for trait-based codegen dispatch anywhere in `cratestack-macros` (verified); List's schema-dependent paged/unpaged branch has no clean home in a fixed trait method signature |

**Recommendation: A**, with one correction to the write-up. `decode` is not just "the message-parsing step" — the per-verb argument list to the dispatch call also varies (Get passes `(id, None)`, Delete `(id)`, Create `(body_bytes)`, Update `(id, patch_bytes)`, List `(raw_query)`), so `decode` must be specified as covering everything from `message` through obtaining `response` (i.e. it includes the `super::axum::#dispatch_ident(...)` call), not merely `message.into_pk()`-style extraction. Otherwise an implementer discovers mid-refactor that "decode" can't be cleanly isolated from "call dispatch" and either invents an undocumented third fragment or bolts the dispatch call onto `build_unary_arm` itself (which then needs a variable-arity call, defeating the point). State the boundary explicitly:

```rust
struct ArmSpec {
    path: proc_macro2::TokenStream,
    request_ty: syn::Ident,
    response_ty: syn::Ident,
    svc_ident: syn::Ident,
    decode: proc_macro2::TokenStream, // message -> ... -> `let response = super::axum::#dispatch_ident(...).await;`
    respond: proc_macro2::TokenStream, // response -> Ok(Response::new(...)) or Err(status)
}
```

Each `build_get_arm`/`build_delete_arm`/`build_create_arm`/`build_update_arm`/`build_list_arm` shrinks to: compute idents, build `decode`/`respond`, call `build_unary_arm(spec)`. Drop the unused `pk: &Field` param from `build_get_arm`, `build_delete_arm`, `build_update_arm`.

### Decision 2 — service.rs sizing (new; the original proposal treated this as a maybe)

Verified line counts at HEAD: module doc = lines 1-64 (64 lines); imports + `build_service` + 3 small helpers (`model_state_from_procedure_state`, `method_path`, `request_prelude`, `status_from_bridge_error`) = lines 65-267 (203 lines, of which `build_service`'s `ApiServer` struct/impl/`into_router` alone is lines 75-203, 129 lines); the five arm builders = lines 268-585 (318 lines).

That means **267 lines of non-arm-builder content already exceed the ~200-LoC ceiling before a single line of `ArmSpec` code exists.** Even a best-case arm-builder consolidation (5 one-line call sites + `ArmSpec` + `build_unary_arm`, call it ~70 lines total) lands service.rs at roughly 64 + 203 + 70 ≈ 337 lines — 68% over the ceiling.

The original proposal's contingency for this ("if still over, split into `service.rs` + `service/procedures.rs`, consistent with... the procedure-arm builders that already live lower in the file") is **factually wrong**: `build_procedure_unary_arm`/`build_procedure_stream_arm` already live in a separate file, `crates/cratestack-macros/src/include/server/grpc/procedure_arms.rs` (185 lines) — confirmed via the `use super::procedure_arms::{...}` import at the top of `service.rs` and the directory listing. There is no procedure-arm code inside `service.rs` to split out; that contingency plan doesn't apply.

**Recommendation: plan the split up front, not as a contingency.** Extract `build_service`'s `ApiServer` struct/impl/`into_router` (lines 75-203, 129 lines — the single largest non-arm chunk) into a sibling module (e.g. `service/router.rs` or `service/api_server.rs`), in the same PR as the `ArmSpec` work. Projected result: `service.rs` ≈ 64 (doc, preserved per AC1's own wording) + 74 (imports + `model_state_from_procedure_state`/`method_path`/`request_prelude`/`status_from_bridge_error`) + ~70 (`ArmSpec` + `build_unary_arm` + 5 shrunk builders) ≈ 208 lines — within the ceiling's stated fuzziness ("~200-LoC ceiling"), versus ~337 lines without this split. Without this, AC1 is at real risk of being silently unmet even though the refactor "worked."

### Decision 3 — Dart/TS descriptor sharing

Correction to the issue body (re-confirmed): the actual shared symbols are `GrpcMessageView` (`grpc/messages.rs:30` in both crates, confirmed `pub(crate)`), `GrpcWireKind` (`grpc/wire.rs`), and the collector fns `collect_message`/`collect_from_fields` + `build_field_descriptor` — not `synth_page_fields`/`build_pb_field` as paraphrased in the issue.

| Option | Breaking | Pros | Cons |
|---|---|---|---|
| **A. New crate `cratestack-client-grpc-shared`** (recommended) | No | Mirrors the precedent CLAUDE.md names for this exact shape — `cratestack-sql` shared by `cratestack-sqlx`/`cratestack-rusqlite`, confirmed via both `Cargo.toml`s. Confirmed exactly two consumers exist for the gRPC descriptor duplication too (`cratestack-client-flutter` and `cratestack-client-rust` have no `GrpcMessageView`/`GrpcWireKind` usage at all, grep-confirmed) — this really is a Dart-vs-TypeScript-only duplication, matching the two-consumer shape claimed. Both moved symbols are already crate-private in their current homes, so the move is genuinely non-breaking | One more workspace member / topo-sort publish entry (mechanical). `cratestack-macros` already depends on `cratestack-proto` directly (confirmed), so any change there — including Option B's new module — forces a rebuild of the proc-macro crate; worth naming as a compile-blast-radius cost of Option B, not framed as "coupling release cadence" (this is a pre-1.0, lockstep-versioned workspace per CLAUDE.md — every crate already ships together regardless of which crate a change lands in) |
| **B. Extend `cratestack-proto` with a new `grpc_view` module** | No | Zero new crate/publish-order entry; thematically adjacent | Broadens a crate whose own doc scopes it to lockfile ownership + `.proto` text emission (confirmed via both `lib.rs`'s and `emit/mod.rs`'s module docs) to a third, unrelated concern (client-language wire descriptors); forces proc-macro-crate rebuilds on unrelated changes |

**Recommendation: A**, unchanged from the original pass. One more thing that needs fixing as part of this: `crates/cratestack-client-dart/src/grpc/wire.rs`'s module doc (lines 1-7, re-verified verbatim) currently justifies the duplication by citing `cratestack-proto::casing`'s "small pure mapping table gets reimplemented per crate" convention. Reading `casing.rs`'s own doc comment (lines 1-7, confirmed) shows that rationale is specifically about `cratestack-proto` not being allowed to depend back on `cratestack-macros` (layering direction, per `docs/design/protobuf.md` §3.3 and CLAUDE.md's crate-layering rule) — it does not apply to two sibling client crates extracting shared logic into a new crate they both already sit above in the dependency graph. That doc comment should be corrected, not just the code.

### Decision 4 — golden-file harness

Both halves have an established pattern in this repo; use them rather than adding `insta` or hand-rolling something new — but one detail needs correcting from the original pass:

- **`service.rs` (macro-expansion level):** follow `crates/cratestack-macros/src/procedure/tests.rs`'s precedent (cratestack#282) — inline `#[test]` fns inside `cratestack-macros` asserting `build_get_arm(...).to_string()` equals **another `quote! { ... }` block's `.to_string()`**, not a hand-captured raw string literal. This matters more here than it did for `procedure/tests.rs`'s ~7-line trait-method example: `service.rs`'s arm bodies run ~50 lines each, and `proc_macro2::TokenStream::to_string()`'s own stringification format (`fn ticks (& self , db : & super :: Cratestack ...)`) is not idiomatic Rust source — a hand-captured literal that size is genuinely hard to review, undercutting the "trivial to review" claim for PR0. Writing the expected side as its own `quote!{}` block (exactly what `procedure/tests.rs` already does) keeps both sides human-readable and keeps the comparison exactly byte-for-byte via the same `.to_string()` normalization on both sides.
- **Dart/TS (generated-file level):** extend the existing `tests/snapshot.rs` harness already in both `cratestack-client-dart` and `cratestack-client-typescript` (checked-in `tests/fixtures`, `CRATESTACK_UPDATE_SNAPSHOTS=1` to refresh, confirmed real) to cover gRPC output, using `examples/grpc-widgets/schemas/widgets.cstack` (confirmed the canonical gRPC fixture, per `generator_grpc.rs`'s own doc comment in both crates) as the new snapshot fixture. Note the existing `generator_grpc.rs` tests are assertion-based, not snapshot-based — this adds a snapshot suite alongside them, it doesn't replace them.

**Sequencing** (per CLAUDE.md's "refactor PRs are scoped per-crate"):

1. **PR0 (test-only, no behavior change):** add the golden `quote!{}`-vs-`quote!{}` `TokenStream::to_string()` assertions to `cratestack-macros`, and the gRPC snapshot fixtures to both client crates, capturing today's output.
2. **PR1 (`cratestack-macros` only):** `ArmSpec` + `build_unary_arm` refactor **and** the `build_service`/`ApiServer` extraction into a sibling module (Decision 2) in the same PR — splitting them across two PRs risks landing PR1 "green" while still failing AC1. Verified green against PR0's golden tests; drop the dead `pk` params.
3. **PR2 (new crate + both client crates):** extract `cratestack-client-grpc-shared`, migrate both generators to consume it, verified byte-identical against PR0's snapshot fixtures. Also correct `wire.rs`'s misattributed module doc.

### Test strategy

- `cargo test -p cratestack-macros` — new inline golden tests (as `quote!{}` comparisons, not string literals) plus existing `ui.rs` trybuild suite.
- `cargo test -p cratestack-client-dart -p cratestack-client-typescript` — new gRPC snapshot fixtures plus existing `generator_grpc.rs` assertion tests (unaffected, still pass against the shared crate's output).
- `just verify-dart` (confirmed present in `justfile:264`) per the issue's own test plan.
- No PG/`CRATESTACK_TEST_DATABASE_URL` needed — pure codegen, no runtime behavior change per the acceptance criteria.
- `just all-checks` before opening either refactor PR, scoped `--workspace --exclude embedded_flutter_native` per CLAUDE.md.
- After PR1, run `wc -l crates/cratestack-macros/src/include/server/grpc/service.rs crates/cratestack-macros/src/include/server/grpc/service/*.rs` (or wherever the split lands) and confirm `service.rs` itself is at/under ~200 lines — add this as an explicit, checked step in PR1's own description, since AC1 is otherwise easy to believe is satisfied by "the diff looks smaller" without actually re-measuring.

## Reviewer notes

What I changed and why, from adversarial re-verification against the actual files (not the triage summary):

1. **Precedent citation fixed (Decision 1).** The original proposal cited `ModelDescriptor`/`TypeScriptGeneratorConfig` as precedent for `ArmSpec`. Neither is the same pattern: `ModelDescriptor` is a *runtime-emitted* metadata constant (part of generated user-facing code), and `TypeScriptGeneratorConfig` is a public generator config struct with plain `String`/`bool`/`Option` fields, no `TokenStream` anywhere. The actually-matching precedent, `ModelHandlerPrep` in `crates/cratestack-macros/src/axum/model/prep.rs`, sat unexamined in the same crate — a per-model struct of ~15 `proc_macro2::TokenStream` fields feeding a shared builder, exactly `ArmSpec`'s shape. This strengthens Option A's case; I swapped the citation.
2. **`decode` fragment boundary corrected (Decision 1).** The per-verb dispatch call has a different argument list per verb (confirmed by reading all five functions), so "decode = message.into_pk() etc." understates what has to live in that field. Spelled out the real boundary (through the `response` binding) so an implementer doesn't get stuck mid-refactor.
3. **New Decision 2, sizing, added.** This is the most consequential fix. Line-counted the file precisely: 267 lines of module-doc + preamble already exceed the ~200-line ceiling before any arm-builder consolidation happens. The original proposal's stated contingency ("split off the procedure-arm builders that already live lower in the file") is checkable and false — those builders already live in a separate file, `procedure_arms.rs`. Replaced the contingency with a concrete, upfront plan (extract `build_service`'s 129-line `ApiServer`/`into_router` scaffold) and the arithmetic showing it's necessary, not optional, for AC1.
4. **Decision 3's "release cadence" con reframed.** `cratestack-macros` does depend on `cratestack-proto` (confirmed), but doesn't currently use any of the types being discussed (confirmed via grep — zero hits), so there's no *logical* coupling from extending `cratestack-proto` — the real cost is proc-macro-crate rebuild/compile-blast-radius, and "release cadence" is a weak framing in a workspace that's already lockstep-versioned pre-1.0. Reframed the con to the accurate mechanism.
5. **Golden-file description corrected (Decision 4).** The cited precedent (`procedure/tests.rs`) compares two `quote!{}.to_string()` outputs, not a hand-captured raw string literal, as the original proposal's phrasing implied. This matters more at `service.rs`'s ~50-line-per-arm scale than it did for the original ~7-line example; corrected so PR0 stays reviewable at the claimed "trivial" bar.
6. **Everything else held up.** The `pk` dead-parameter analysis, the two-consumer framing for Dart/TS (checked `cratestack-client-flutter` and `cratestack-client-rust` for the same duplication — neither has it), the `casing.rs` layering-rationale correction, the non-breaking judgments, `just verify-dart`'s existence, and the snapshot/`CRATESTACK_UPDATE_SNAPSHOTS` pattern all check out against the actual files and are preserved as originally proposed.

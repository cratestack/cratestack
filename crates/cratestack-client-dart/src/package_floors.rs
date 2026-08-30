//! The pub.dev version requirements a generated Dart client declares for
//! CrateStack's own published Dart packages (cratestack#754).
//!
//! # The rule
//!
//! > A generated dependency constraint states an **API compatibility
//! > requirement**. It is never derived from `CARGO_PKG_VERSION`, at any
//! > precision.
//!
//! This is not new doctrine — `docs/tooling/dart-publishing.md` already
//! states it for `cratestack_builder`'s own dependency on
//! `cratestack_annotations` ("The lower bound states an API requirement,
//! not a version relationship … Raise this constraint only when the
//! builder begins using a newly-added annotation field. Never as part of
//! a routine version bump."). It had simply never been applied to what
//! the *generator* emits.
//!
//! # Why a constant, and not a floor derived from the current version
//!
//! `just bump` moves the workspace version *before* the corresponding
//! pub.dev publish, which runs off the pushed tag — so on a bump PR the
//! workspace is already at `0.8.14` while pub.dev's newest is still
//! `0.8.13`. Anything derived from `CARGO_PKG_VERSION` therefore names an
//! unpublished version for the whole window:
//!
//! - An **exact pin** (`^{CARGO_PKG_VERSION}`, what this emitted before
//!   #754) breaks on *every* bump. It took down `Prepare Release` for
//!   0.8.14 — five snapshot fixtures, three `flutter pub get` tests, and
//!   `just regen-examples --check`, all at once.
//! - A **minor floor** (`^{major}.{minor}.0`, what
//!   `cratestack-client-typescript` emits today for `@cratestack/cbor`)
//!   survives a patch bump but still *moves* at a minor bump, so it has a
//!   residual window its own doc comment states honestly.
//! - A **constant** does not move at all, so it can never name an
//!   unpublished version, at any bump size.
//!
//! The wider consequence is the one that matters: generator output stops
//! being a function of the release version. That is what makes the
//! committed snapshots and example clients stable across a bump, rather
//! than something `just bump` silently invalidates.
//!
//! The caret ceiling (`^0.8.10` is `>=0.8.10 <0.9.0` on a `0.x` version —
//! pub pins the *second* component pre-1.0) is deliberate and is *not* a
//! release-safety concern: after 0.9.0 ships, a generated client keeps
//! resolving 0.8.x until the floor is deliberately raised. That is
//! staleness, not breakage, and raising it is exactly the considered act
//! the rule prescribes — as opposed to a forward-compatibility promise
//! across a pre-1.0 minor that nothing here could back.
//!
//! # Keeping these honest
//!
//! A comment is not a control, and this repo has the receipt: the
//! previous hand-maintained floor
//! (`dart-packages/cratestack_builder/pubspec.yaml`) read `^0.8.8`,
//! justified as "the first release with `touchFlagFields`/
//! `nonDefaultingListFields`". Checked against pub.dev, **0.8.8 was never
//! published** (versions run 0.8.7 -> 0.8.10) and **0.8.7 does not
//! contain those fields** — 0.8.10 is the first that does. It was
//! harmless only because a caret resolves upward. So these constants are
//! backed by two things that can fail instead:
//!
//! 1. `package_floors_tests.rs` (a sibling unit-test module) asserts
//!    the floor emitted here is at least the floor
//!    `dart-packages/cratestack_builder/pubspec.yaml` declares, read
//!    from that file — so the generator can never ask for less than the
//!    builder it invokes requires — and that both floors sit strictly
//!    below the current, not-yet-published workspace version.
//! 2. CI's `flutter (flutter-riverpod example)` job resolves the
//!    committed client *at these exact versions* via a generated
//!    `pubspec_overrides.yaml` before running `build_runner`, so a floor
//!    that is too low fails there rather than at a user's build.
//!
//! Raise these when the generator starts emitting an annotation argument
//! (or relying on builder behaviour) that the floor release does not
//! have — and only then.

/// `cratestack_annotations` — the runtime annotation package a generated
/// client lists under `dependencies:`.
///
/// `0.8.10` is the first *published* release carrying the
/// `touchFlagFields` and `nonDefaultingListFields` arguments this
/// generator emits on `@CratestackBuilder(...)`. Verified against the
/// published archives, not the changelog: 0.8.5/0.8.6/0.8.7 do not
/// contain either identifier, and 0.8.8/0.8.9 do not exist on pub.dev.
///
/// Raised `^0.8.10` -> `^0.9.1` on 2026-08-30. **Not** because the
/// generator started emitting a newer annotation argument — it did not —
/// but because the caret ceiling had begun excluding published releases.
/// `^0.8.10` is `>=0.8.10 <0.9.0`, and every `cratestack_*` Dart package
/// is on pub.dev at 0.9.1, so a generated client could no longer resolve
/// the current release at all. This is the "deliberately raised" act this
/// module's doc prescribes for exactly that situation.
pub(crate) const CRATESTACK_ANNOTATIONS_FLOOR: &str = "^0.9.1";

/// `cratestack_builder` — the `source_gen` builder a generated client
/// lists under `dev_dependencies:`, run by `build_runner` to expand
/// `@CratestackBuilder(...)` into `{Class}Builder` parts.
///
/// `0.8.10` is the first *published* release that actually reads the two
/// arguments above off the annotation; 0.8.7 does not, and declares
/// `cratestack_annotations: ^0.8.5`. A generated client resolving an
/// older builder would silently produce builders that disagree with the
/// schema rather than failing at `pub get`.
///
/// Raised `^0.8.10` -> `^0.9.1` on 2026-08-30, with
/// `CRATESTACK_ANNOTATIONS_FLOOR` and for the same reason.
///
/// These two cannot move independently, and the third piece is NOT in this
/// file: `dart-packages/cratestack_builder/pubspec.yaml` declares its own
/// `cratestack_annotations` constraint, which was also `^0.8.10`. Raising
/// only the generator's floors leaves the published builder forbidding the
/// annotations release the generator now asks for —
///
/// ```text
/// Because cratestack_builder >=0.8.14 depends on cratestack_annotations
/// ^0.8.10 and <client> depends on cratestack_annotations ^0.9.1,
/// cratestack_builder >=0.8.14 is forbidden.
/// ```
///
/// — which `computed_params_wire_equality` catches by running a real
/// `flutter pub get`. That test is the reason this change is complete
/// rather than half-applied.
pub(crate) const CRATESTACK_BUILDER_FLOOR: &str = "^0.9.1";

/// `cratestack_cbor` — the native CBOR codec a generated client lists
/// under `dependencies:` when `native_cbor` is on (the default;
/// `--no-native-cbor` opts out). cratestack#779.
///
/// `0.8.0` is the earliest release published to pub.dev at all (the
/// version list runs `0.8.0, 0.8.2, 0.8.3, …` — there is no `0.7.x`), and
/// it already carries the entire surface a generated runtime touches:
/// `createCborCodec()` returning `Future<CratestackCborCodec>`, plus that
/// class's `encodeJson(String)`/`decodeJson(List<int>)`. Verified by
/// unpacking the published archives for 0.8.0/0.8.5/0.8.9/0.8.14 and
/// grepping the signatures out of `lib/`, not by reading the changelog —
/// the method #754 established after the hand-written `^0.8.8` floor
/// turned out to name a version pub.dev never had.
///
/// So this floor was bounded by what exists, not by what the generator
/// needs. The caret ceiling is what made it useful anyway — `^0.8.0` is
/// `>=0.8.0 <0.9.0`, so it resolved the newest 0.8.x on the day the user
/// ran `pub get`, which is how a generated client picked up #794's
/// idempotent `createCborCodec()` without this constant moving.
///
/// **That stopped working the day 0.9.1 published**, which is why this is
/// now `^0.9.1`. "Resolves the newest on the day you run `pub get`"
/// quietly became "resolves the newest 0.8.x, forever". It surfaced from a
/// consumer (`vaam-store/mobile`, cratestack#838) as a hard resolution
/// failure rather than as staleness, because a workspace depending on both
/// a generated client and `cratestack_cbor` directly cannot ask for 0.9.1
/// at all:
///
/// ```text
/// Because vymalo depends on vendor_client from path which depends on
/// cratestack_cbor ^0.8.0, cratestack_cbor ^0.8.0 is required.
/// So, because vymalo depends on cratestack_cbor ^0.9.1, version solving
/// failed.
/// ```
///
/// That generated client came from cratestack-cli 0.9.1 itself — a 0.9.1
/// generator emitting a constraint excluding its own release's packages.
///
/// The raise costs callers nothing beyond requiring a currently-published
/// release: 0.9.1 is API-identical to 0.8.15 for consumers. The diff across
/// `dart-packages/cratestack_cbor/lib/` between those two tags is doc
/// comments, a `lints` dev-dependency and podspec versions.
///
/// **Not** the fix for #798's retry behaviour: that lives in the
/// generated runtime itself (`rest-runtime.dart.j2` /
/// `rpc_runtime/types.dart.j2`), which clears its own cache on failure
/// and therefore needs nothing from this package's version.
pub(crate) const CRATESTACK_CBOR_FLOOR: &str = "^0.9.1";

#[cfg(test)]
#[path = "package_floors_tests.rs"]
mod package_floors_tests;

//! The npm version requirements a generated TypeScript client declares
//! for CrateStack's own published packages (cratestack#779).
//!
//! # The rule
//!
//! > A generated dependency constraint states an **API compatibility
//! > requirement**. It is never derived from `CARGO_PKG_VERSION`, at any
//! > precision.
//!
//! Established by cratestack#754 and implemented there for the Dart
//! generator's two sites — see
//! `crates/cratestack-client-dart/src/package_floors.rs`, whose module
//! doc carries the full reasoning and is deliberately not restated here.
//! This module is the TypeScript half, closing the remaining two sites
//! #754 scoped out.
//!
//! # Why a constant, and not the `^{major}.{minor}.0` this replaced
//!
//! `native_cbor_version_requirement` used to call a
//! `minor_floor_version_requirement(env!("CARGO_PKG_VERSION"))` helper,
//! introduced by #746 as a partial fix. Its own doc comment stated the
//! residual gap honestly: a minor floor is still *derived from the
//! current version*, so it moves to `^0.9.0` at the 0.9.0 bump and names
//! an unpublished package for that whole window. It narrowed the failure
//! from "every bump" to "every minor bump" rather than closing it.
//!
//! A constant does not move at all, so it can never name an unpublished
//! version, at any bump size. The wider consequence is the one that
//! matters: generator output stops being a function of the release
//! version, which is what makes the committed snapshots and example
//! clients stable across a bump.
//!
//! Note this is a no-op for the emitted string *today*: at workspace
//! version `0.8.14`, `minor_floor_version_requirement` already produced
//! `^0.8.0`. The change is that it will keep producing `^0.8.0` at
//! `0.9.0`, instead of following the bump into a version npm cannot yet
//! serve.
//!
//! # The caret ceiling is deliberate
//!
//! npm resolves `^0.8.0` on a `0.x` version as `>=0.8.0 <0.9.0` — the
//! *second* component is pinned pre-1.0, exactly as pub does. So a
//! generated client resolves the newest `0.8.x` on the day the user runs
//! `npm install`, and after `0.9.0` ships it keeps resolving `0.8.x`
//! until the floor is deliberately raised. That is staleness, not
//! breakage, and raising it is the considered act the rule prescribes.
//!
//! It is also why neither floor below sits in the `0.7.x` line even
//! though both APIs predate `0.8.0`: `^0.7.16` would be `>=0.7.16
//! <0.8.0` and would pin every generated client *off* the current
//! release line entirely.
//!
//! # Keeping these honest
//!
//! #754's receipt applies verbatim: a hand-maintained floor there read
//! `^0.8.8`, a version pub.dev never published, and every offline check
//! available was satisfied by it. So neither constant below is justified
//! from a changelog. Both were verified by unpacking the actual
//! published tarballs off the npm registry and grepping the shipped
//! `dist/*.d.ts` for the exact identifiers the templates reference.
//!
//! Unlike the Dart floors, there is no in-repo declared bound to derive
//! these from — `packages/cratestack-refine` and
//! `packages/cratestack-cbor-*` carry the lockstep workspace version,
//! not a floor, so there is no `cratestack_builder`-style pubspec to
//! read a requirement out of. The guards are therefore:
//!
//! 1. `package_floors_tests.rs` asserts each floor is strictly below the
//!    current, not-yet-published workspace version, and that neither is
//!    `CARGO_PKG_VERSION` at any precision. That is what a well-meaning
//!    "keep it in sync with the bump" change would trip.
//! 2. CI's `js (react-vite-swr example)` job installs a generated client
//!    *at these exact versions* and typechecks it, so a floor that is
//!    too low — or names a version npm cannot serve — fails there rather
//!    than at a user's `npm install`.

/// `@cratestack/refine` — the refine data-provider package a generated
/// client lists under both `peerDependencies` and `devDependencies` when
/// `--refine` is on.
///
/// `0.8.0` is the earliest release in the current `0.8.x` line, and it
/// carries everything `refine.ts.j2` references: the `ResourceMap` and
/// `RpcResourceMap` exported types, and the `primaryKey` / `paged` /
/// `versionField` entry fields the generated `cratestackRefineResources()`
/// populates. Verified against the published tarballs for every `0.7.16`
/// through `0.8.14`, not against the changelog. (`0.7.16` is where that
/// surface actually first appears — `0.7.14` has neither `ResourceMap`
/// nor `RpcResourceMap` — but see the module doc for why the floor does
/// not go there.)
pub(crate) const CRATESTACK_REFINE_FLOOR: &str = "^0.8.0";

/// `@cratestack/cbor` — the native RPC codec a generated client lists
/// under `dependencies` when `native_cbor` is on (the default) *and* the
/// schema is `transport rpc`. A REST client never gets this dependency
/// at all; `rest-runtime.ts.j2` has no codec seam.
///
/// `0.8.0` is the earliest release in the current `0.8.x` line. The one
/// import `rpc-runtime.ts.j2` makes — `createCborCodec()` returning a
/// `Promise<CratestackRpcCodec>`, which `resolveCodec()`'s memoize-and-
/// retry depends on being a promise — is present in every published
/// tarball checked back to `0.7.10`, so the floor is bounded by the
/// release line, not by the API.
///
/// **Known gap, stated rather than hidden.** No *published*
/// `@cratestack/cbor` encodes a `Uint8Array` as a CBOR byte string. As
/// of `0.8.14` it still walks it as a plain object, so a `Bytes` field
/// reaches the wire as a CBOR map (`{"0":1,"1":2,"2":3}`) that no
/// server-side `Vec<u8>` can decode — measured directly against
/// `npm i @cratestack/cbor@0.8.14`, not inferred. cratestack#783/#787
/// fixed that, but the fix has not shipped: `0.8.14` published on
/// 2026-08-27 and #787 merged on 2026-08-28.
///
/// The honest floor for a `Bytes`-carrying schema is therefore the first
/// release containing #787, and this constant must be raised to it once
/// that release publishes. It is **not** raised pre-emptively: naming an
/// unpublished version is the exact defect #754 and this ticket exist to
/// remove, and it would break every `npm install` today — including this
/// crate's own `native_cbor_decimal_encode` test, which installs the real
/// package from the registry. Tracked as a follow-up rather than
/// silently absorbed here; note this gap is unchanged by #779, since the
/// `^{major}.{minor}.0` this replaced already emitted `^0.8.0`.
pub(crate) const CRATESTACK_CBOR_FLOOR: &str = "^0.8.0";

#[cfg(test)]
#[path = "package_floors_tests.rs"]
mod package_floors_tests;

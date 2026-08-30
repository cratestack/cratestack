//! Shared Cargo-feature enforcement for `.cstack` `extension <name> { }`
//! declarations (cratestack#161) — the *one* place every extension
//! registers itself against this mechanism, reused unmodified by all three
//! entry macros. Split out of `parse.rs`/co-located with
//! `datasource_guard.rs` per the repo's 200-LoC file convention; the shape
//! deliberately mirrors that module, since it already solves the identical
//! problem ("is this schema-declared capability actually available in the
//! crate compiling it") for `db = Postgres`.
//!
//! `cratestack-parser` (cratestack#153) already turns `extension pgvector
//! { }` into `Schema.declared_extensions: BTreeSet<ExtensionKind>` — a
//! schema-visible statement of intent, parsed and validated the same
//! regardless of what the consuming crate was compiled with. That's layer 1
//! of `docs/design/extensions.md` §2. This module is layer 2: it decides
//! whether the *code* for a declared extension actually exists in this
//! build, by checking a same-named Cargo feature on `cratestack-macros`
//! itself.
//!
//! **Why `cfg!(feature = "...")` against this crate's own features, and not
//! `CARGO_FEATURE_<NAME>` env vars naming the *consumer's* features** (the
//! mechanism originally proposed in issue #161 and in the ticket that
//! tracks it): verified empirically while building this ticket — see
//! `docs/design/extensions.md` §2's "revised after implementation" note —
//! `CARGO_FEATURE_<NAME>` is set by Cargo for build-script invocations
//! only. A proc-macro expands *inside the `rustc` process compiling the
//! invoking crate*, which never receives those variables; reading them here
//! would just always see nothing. What actually works, and what
//! `datasource_guard.rs`'s `postgres` guard already does: `cratestack-macros`
//! declares the same-named Cargo
//! feature itself (`rate_limit`, `pgvector`), and a facade crate
//! (`cratestack-pg`, `cratestack-sqlite`) forwards its own feature of the
//! same name down via `rate_limit = ["cratestack-macros/rate_limit"]` /
//! `pgvector = ["cratestack-macros/pgvector"]` in its `Cargo.toml`. Cargo's
//! ordinary feature-unification then means enabling the feature on the
//! facade an app actually depends on transitively enables it on
//! `cratestack-macros` too — the same technique `sqlx`/`sqlx-macros` use for
//! exactly this kind of macro-visible feature gate. That forwarding wiring
//! is each extension's own follow-up ticket (cratestack#154 for
//! `rate_limit`, cratestack#155 for `pgvector`); this module only needs the
//! feature to exist on `cratestack-macros`, declared in its own
//! `Cargo.toml`.
//!
//! **Registering a new extension** against this mechanism means adding one
//! arm each to [`required_feature`] and [`feature_enabled`] (`cfg!` needs a
//! string literal, so the feature table can't be a runtime-computed map)
//! plus a same-named feature in `cratestack-macros/Cargo.toml` — nothing
//! else in this file, or in any of the three composers below, needs to
//! change.

#[cfg(test)]
mod tests;

mod errors;

use self::errors::{embedded_postgres_only_error, missing_feature_error};

use proc_macro::TokenStream;
use syn::LitStr;

use cratestack_core::{ExtensionKind, Schema};

/// Cargo feature name required for `kind`, and the tracking issue that
/// wires up its facade-forwarding (surfaced in the diagnostic so a
/// developer hitting this error has somewhere to check current status).
/// The single per-extension registration point this module exists to
/// centralize.
fn required_feature(kind: ExtensionKind) -> (&'static str, u32) {
    match kind {
        ExtensionKind::RateLimit => ("rate_limit", 154),
        ExtensionKind::Pgvector => ("pgvector", 155),
        ExtensionKind::Postgis => ("postgis", 842),
    }
}

/// Whether `kind` is inherently a Postgres-server capability, and so can
/// never be valid under `include_embedded_schema!` regardless of Cargo
/// features — the rusqlite backend has no `CREATE EXTENSION` and no
/// matching column types to map onto.
fn is_postgres_only(kind: ExtensionKind) -> bool {
    match kind {
        ExtensionKind::Pgvector | ExtensionKind::Postgis => true,
        ExtensionKind::RateLimit => false,
    }
}

/// Whether `kind`'s required Cargo feature is enabled on *this* crate
/// (`cratestack-macros`) — see the module doc for why this, and not a
/// consumer-side `CARGO_FEATURE_*` env var, is the right check. `cfg!`
/// requires a string literal per arm, so this can't be derived from
/// [`required_feature`]'s table at runtime.
fn feature_enabled(kind: ExtensionKind) -> bool {
    match kind {
        ExtensionKind::RateLimit => cfg!(feature = "rate_limit"),
        ExtensionKind::Pgvector => cfg!(feature = "pgvector"),
        ExtensionKind::Postgis => cfg!(feature = "postgis"),
    }
}

/// Pure predicate: the first extension `schema` declared whose Cargo
/// feature is off, in `declared_extensions`' stable (`BTreeSet`) order —
/// the condition every guard below branches on. Kept separate from the
/// guards themselves (which return `proc_macro::TokenStream` and panic
/// outside a real proc-macro invocation — see `datasource_guard.rs`'s tests
/// for the same constraint) so it stays directly unit-testable.
fn first_missing_extension(schema: &Schema) -> Option<ExtensionKind> {
    schema
        .declared_extensions
        .iter()
        .copied()
        .find(|kind| !feature_enabled(*kind))
}

/// Ordinary feature check, shared by [`guard_server_declared_extensions`]
/// and [`guard_client_declared_extensions`]: every declared extension must
/// have its matching Cargo feature enabled, no exceptions. Also used by
/// [`guard_embedded_declared_extensions`] for every extension *other than*
/// `pgvector`, which that guard rejects unconditionally first.
fn guard_declared_extensions(schema_path: &LitStr, schema: &Schema) -> Result<(), TokenStream> {
    match first_missing_extension(schema) {
        Some(kind) => Err(missing_feature_error(schema_path, kind)),
        None => Ok(()),
    }
}

/// `include_server_schema!` only.
pub(super) fn guard_server_declared_extensions(
    schema_path: &LitStr,
    schema: &Schema,
) -> Result<(), TokenStream> {
    guard_declared_extensions(schema_path, schema)
}

/// `include_client_schema!` only. Same policy as the server guard today —
/// nothing about `rate_limit`/`pgvector` currently differs for the client
/// role; a future extension whose client-side story diverges gets its own
/// guard here rather than a branch threaded into this one shared by both
/// roles.
pub(super) fn guard_client_declared_extensions(
    schema_path: &LitStr,
    schema: &Schema,
) -> Result<(), TokenStream> {
    guard_declared_extensions(schema_path, schema)
}

/// `include_embedded_schema!` only. A Postgres-only extension
/// (`pgvector`, `postgis`) is unconditionally invalid here, feature or
/// no feature: the embedded backend is rusqlite-only, so no Cargo
/// feature could ever make `Vector(n)` or `Geography(...)` valid
/// against it — see `docs/design/extensions.md` §6/§6b. Every other
/// declared extension falls through to the ordinary feature check.
pub(super) fn guard_embedded_declared_extensions(
    schema_path: &LitStr,
    schema: &Schema,
) -> Result<(), TokenStream> {
    if let Some(kind) = schema
        .declared_extensions
        .iter()
        .copied()
        .find(|kind| is_postgres_only(*kind))
    {
        return Err(embedded_postgres_only_error(schema_path, kind));
    }
    guard_declared_extensions(schema_path, schema)
}

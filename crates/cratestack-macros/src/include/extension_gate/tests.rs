//! Unit tests for `extension_gate.rs`'s pure predicates. Split into its own
//! file per the repo's 200-LoC convention (same shape as
//! `include/server/runtime/tests.rs`) rather than an inline `mod tests`.
//!
//! The guard functions themselves (`guard_server_declared_extensions` and
//! friends) return `proc_macro::TokenStream` and call
//! `syn::Error::to_compile_error()`, which panics outside a real
//! proc-macro invocation context — same constraint `datasource_guard.rs`'s
//! tests document for its own guards. So these
//! tests exercise the pure predicates the guards are built from
//! (`first_missing_extension`, `required_feature`, `feature_enabled`)
//! directly; the guards' actual compile-time behavior (including both
//! diagnostic messages' exact wording) is covered end-to-end by
//! `crates/cratestack-macros/tests/ui.rs`'s trybuild compile-fail cases.

use cratestack_core::ExtensionKind;

#[cfg(any(feature = "rate_limit", feature = "pgvector", feature = "postgis"))]
use super::feature_enabled;
use super::{first_missing_extension, required_feature};

fn parse(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("schema should parse")
}

#[test]
fn no_declared_extensions_has_no_missing_feature() {
    let schema = parse(
        r#"
model Widget {
  id Int @id
}
"#,
    );
    assert!(schema.declared_extensions.is_empty());
    assert_eq!(first_missing_extension(&schema), None);
}

#[test]
#[cfg(not(any(feature = "rate_limit", feature = "pgvector", feature = "postgis")))]
fn declared_extension_without_its_feature_is_flagged() {
    // Only meaningful with neither feature enabled — this crate's own
    // default-feature test run — so both are reported missing,
    // exercising the "declared, feature off" branch this ticket's
    // acceptance criteria call out explicitly. CI also runs this
    // suite with `--features rate_limit` and `--features pgvector`
    // (see the two `_feature_enabled_is_reflected` tests below), where
    // this test is compiled out rather than failing on the now-enabled
    // half.
    let schema = parse(
        r#"
extension rate_limit {
}

model Widget {
  id Int @id
}
"#,
    );
    assert_eq!(
        first_missing_extension(&schema),
        Some(ExtensionKind::RateLimit)
    );

    let schema = parse(
        r#"
extension pgvector {
}

model Widget {
  id Int @id
}
"#,
    );
    assert_eq!(
        first_missing_extension(&schema),
        Some(ExtensionKind::Pgvector)
    );
}

#[test]
#[cfg(not(any(feature = "rate_limit", feature = "pgvector", feature = "postgis")))]
fn missing_extension_is_reported_in_declared_extensions_stable_order() {
    // `declared_extensions` is a `BTreeSet<ExtensionKind>`, so
    // iteration order follows `ExtensionKind`'s derived `Ord` (variant
    // declaration order: `RateLimit` before `Pgvector`) regardless of
    // the order the two `extension` blocks appear in source. Only
    // meaningful with neither feature enabled, same as the test above.
    let schema = parse(
        r#"
extension pgvector {
}

extension rate_limit {
}

model Widget {
  id Int @id
}
"#,
    );
    assert_eq!(
        first_missing_extension(&schema),
        Some(ExtensionKind::RateLimit)
    );
}

#[test]
fn required_feature_names_every_extension_kind() {
    for kind in ExtensionKind::ALL {
        let (feature, tracking_issue) = required_feature(kind);
        assert!(!feature.is_empty());
        assert!(tracking_issue > 0);
    }
}

// The `cfg!(feature = "...")` branch inside `feature_enabled` for each
// extension is exercised by CI running this crate's test suite twice
// more — with `--features rate_limit` and with `--features pgvector` —
// per this ticket's verification checklist. These two tests only run
// under the matching feature, confirming `feature_enabled` flips to
// `true` (and so `first_missing_extension` no longer reports it) rather
// than being hardcoded `false`.
#[test]
#[cfg(feature = "rate_limit")]
fn rate_limit_feature_enabled_is_reflected() {
    assert!(feature_enabled(ExtensionKind::RateLimit));
    let schema = parse(
        r#"
extension rate_limit {
}

model Widget {
  id Int @id
}
"#,
    );
    assert_eq!(first_missing_extension(&schema), None);
}

#[test]
#[cfg(feature = "pgvector")]
fn pgvector_feature_enabled_is_reflected() {
    assert!(feature_enabled(ExtensionKind::Pgvector));
    let schema = parse(
        r#"
extension pgvector {
}

model Widget {
  id Int @id
}
"#,
    );
    assert_eq!(first_missing_extension(&schema), None);
}

#[test]
#[cfg(feature = "postgis")]
fn postgis_feature_enabled_is_reflected() {
    assert!(feature_enabled(ExtensionKind::Postgis));
    let schema = parse(
        r#"
extension postgis {
}

model DeliveryZone {
  id Int @id
  serviceArea Geography(Polygon, 4326)
}
"#,
    );
    assert_eq!(first_missing_extension(&schema), None);
}

/// PostGIS is Postgres-only in the same sense pgvector is, so the
/// embedded guard must reject it whether or not the Cargo feature is
/// on. Guards the generalisation of that guard from a `pgvector`
/// special case to `is_postgres_only`.
#[test]
fn postgis_is_postgres_only() {
    assert!(super::is_postgres_only(ExtensionKind::Postgis));
    assert!(super::is_postgres_only(ExtensionKind::Pgvector));
    assert!(!super::is_postgres_only(ExtensionKind::RateLimit));
}

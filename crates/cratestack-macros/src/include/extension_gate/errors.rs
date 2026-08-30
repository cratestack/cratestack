//! `compile_error!` constructors for the extension gate.
//!
//! Split from `extension_gate.rs` (200-LoC ceiling): that module owns
//! the per-extension registry and the three role guards; this one owns
//! only the diagnostic text those guards emit. The messages are long by
//! design — a developer hitting one needs to know which layer failed
//! (declaration vs Cargo feature) and what to do about it — which is
//! exactly why they earn their own file.

use proc_macro::TokenStream;
use syn::LitStr;

use cratestack_core::ExtensionKind;

use super::required_feature;

pub(super) fn missing_feature_error(schema_path: &LitStr, kind: ExtensionKind) -> TokenStream {
    let name = kind.as_str();
    let (feature, tracking_issue) = required_feature(kind);
    TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            format!(
                "schema declares `extension {name} {{ }}`, but `cratestack-macros` was compiled \
                 without its `{feature}` Cargo feature enabled. Declaring an extension only \
                 unlocks schema syntax (docs/design/extensions.md §2, layer 1); the matching \
                 Cargo feature is what makes the supporting code exist in this build at all \
                 (layer 2) — enable `{feature}` on the facade crate you depend on (e.g. \
                 `cratestack = {{ package = \"cratestack-pg\", features = [\"{feature}\"] }}`) \
                 once that facade forwards it (tracking: \
                 https://github.com/cratestack/cratestack/issues/{tracking_issue}), or drop the \
                 `extension {name} {{ }}` block if this schema doesn't actually need it."
            ),
        )
        .to_compile_error(),
    )
}

pub(super) fn embedded_postgres_only_error(
    schema_path: &LitStr,
    kind: ExtensionKind,
) -> TokenStream {
    let name = kind.as_str();
    let (scalar, column_type, section) = match kind {
        ExtensionKind::Pgvector => ("`Vector(n)`", "`vector(n)`", "§6"),
        ExtensionKind::Postgis => (
            "`Geography(...)`/`Geometry(...)`",
            "`geography`/`geometry`",
            "§6b",
        ),
        // `is_postgres_only` gates every caller, so no other kind
        // reaches here.
        ExtensionKind::RateLimit => {
            unreachable!("rate_limit is not a Postgres-only extension and never reaches this error")
        }
    };
    TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            format!(
                "schema declares `extension {name} {{ }}`, but `include_embedded_schema!` can \
                 never support it, no matter which Cargo features are enabled — {name} is a \
                 Postgres extension and the embedded backend is rusqlite-only, so there is no \
                 {column_type} column type or `CREATE EXTENSION` for it to map onto. Use \
                 `include_server_schema!` instead (with its `{name}` Cargo feature enabled) for \
                 schemas that need {scalar} fields, or drop the `extension {name} {{ }}` block \
                 from schemas meant for the embedded backend. See \
                 docs/design/extensions.md {section}."
            ),
        )
        .to_compile_error(),
    )
}

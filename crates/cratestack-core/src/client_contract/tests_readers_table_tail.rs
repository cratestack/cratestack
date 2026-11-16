//! The drop list's readers from `@isolation` on (moved here for the
//! file-length ceiling). See `tests_readers_table.rs`.

pub(super) const TAIL_READERS: &[(&str, &[&str])] = &[
    (
        "@isolation",
        &[
            "cratestack-macros/src/include/server/isolated.rs",
            "cratestack-macros/src/shared/procedure_attrs.rs",
            "cratestack-sqlx/src/bound.rs",
            "cratestack-sqlx/src/bound/deferred.rs",
            "cratestack-sqlx/src/bound/join.rs",
            "cratestack-sqlx/src/bound/nested.rs",
            "cratestack-sqlx/src/isolated_run.rs",
        ],
    ),
    // Read through `cratestack_core::schema::authn_attribute`; the LSP
    // only shows the attribute in its completion popup.
    ("@authn", &["cratestack-lsp/src/authn_completion.rs"]),
    // Read through `schema::device_auth_attribute`; the LSP shows it in completion.
    // `cratestack-client-dart/src/config.rs` names it only in the
    // `FespalierRefNotATag` error: the Dart generator gates an extra client-side
    // helper class and the pubspec on it, and no wire type, route, op id or
    // decode moves.
    (
        "@device_auth",
        &[
            "cratestack-client-dart/src/config.rs",
            "cratestack-lsp/src/device_auth_completion.rs",
        ],
    ),
    (
        "@length",
        &[
            "cratestack-macros/src/validators/parse.rs",
            "cratestack-migrate/src/convert/checks.rs",
            "cratestack-mock-wiremock/src/field_value.rs",
            "cratestack-studio/src/validators/predicates.rs",
        ],
    ),
    (
        "@range",
        &[
            "cratestack-macros/src/validators/emit.rs",
            "cratestack-macros/src/validators/parse.rs",
            "cratestack-migrate/src/convert/checks.rs",
            "cratestack-studio/src/validators/predicates.rs",
        ],
    ),
    (
        "@regex",
        &[
            "cratestack-macros/src/validators/emit.rs",
            "cratestack-macros/src/validators/parse.rs",
            "cratestack-mock-wiremock/src/field_value.rs",
            "cratestack-studio/src/validators/predicates.rs",
        ],
    ),
    (
        "@email",
        &[
            "cratestack-mock-wiremock/src/field_value.rs",
            "cratestack-studio/src/validators/predicates.rs",
        ],
    ),
    (
        "@uri",
        &[
            "cratestack-mock-wiremock/src/field_value.rs",
            "cratestack-studio/src/validators/predicates.rs",
        ],
    ),
    (
        "@iso4217",
        &[
            "cratestack-migrate/src/convert/checks.rs",
            "cratestack-mock-wiremock/src/field_value.rs",
            "cratestack-studio/src/validators/predicates.rs",
        ],
    ),
    ("@pii", &["cratestack-macros/src/shared/attrs.rs"]),
    ("@sensitive", &["cratestack-macros/src/shared/attrs.rs"]),
    ("@db_enforce", &["cratestack-migrate/src/convert/checks.rs"]),
    (
        "@unique",
        &[
            "cratestack-macros/src/model/descriptor/unique.rs",
            "cratestack-migrate/src/convert/fields.rs",
            "cratestack-migrate/src/convert/renames.rs",
        ],
    ),
    ("@from", &[]),
    (
        "@deprecated",
        &["cratestack-macros/src/axum/procedure/route_attrs.rs"],
    ),
];

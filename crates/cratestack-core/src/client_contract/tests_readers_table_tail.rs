//! The drop list's readers from `@length` on. See `tests_readers_table.rs`.

pub(super) const TAIL_READERS: &[(&str, &[&str])] = &[
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

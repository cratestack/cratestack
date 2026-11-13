//! The drop list's readers from `@pii` on. See `tests_readers_table.rs`.

pub(super) const TAIL_READERS: &[(&str, &[&str])] = &[
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

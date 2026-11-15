//! A reader tripwire for the storage names (cratestack#1375). `@@map` and
//! `@map` are on the drop list because they move only SQL, never a route, op
//! id, wire field or client. That holds while nothing that shapes those reads
//! `Model::sql_table_name`, `Field::sql_column_name` or
//! `Model::sql_column_of`; this pins, per helper, the source files that call
//! it (same scan and exclusions as `tests_readers.rs`). A new caller fails
//! here: check that it builds SQL, not a route, identifier or client, then
//! update the table.

use std::path::Path;

use super::tests_readers::readers_of;

/// `(helper, files that call it)`.
const STORAGE_READERS: &[(&str, &[&str])] = &[
    (
        "sql_table_name",
        &[
            "cratestack-macros/src/model/descriptor.rs",
            "cratestack-macros/src/relation/types.rs",
            "cratestack-macros/src/view/descriptor.rs",
            "cratestack-migrate/src/convert.rs",
            "cratestack-migrate/src/convert/relations.rs",
            "cratestack-migrate/src/diff/views.rs",
            "cratestack-studio/src/api/export.rs",
            "cratestack-studio/src/data/model_info.rs",
        ],
    ),
    (
        "sql_column_name",
        &[
            "cratestack-macros/src/model/descriptor.rs",
            "cratestack-macros/src/model/descriptor/columns.rs",
            "cratestack-macros/src/model/descriptor/defaults.rs",
            "cratestack-macros/src/model/descriptor/embedded.rs",
            "cratestack-macros/src/model/descriptor/generated.rs",
            "cratestack-macros/src/model/descriptor/unique.rs",
            "cratestack-macros/src/model/field_module.rs",
            "cratestack-macros/src/model/row_pg.rs",
            "cratestack-macros/src/model/row_sqlite.rs",
            "cratestack-macros/src/policy/model/comparison.rs",
            "cratestack-macros/src/policy/model/in_list.rs",
            "cratestack-macros/src/policy/model/relation_path.rs",
            "cratestack-macros/src/policy/model/term.rs",
            "cratestack-macros/src/relation/flat.rs",
            "cratestack-macros/src/relation/order_catalog.rs",
            "cratestack-macros/src/relation/types.rs",
            "cratestack-macros/src/shared/sql.rs",
            "cratestack-macros/src/transport/subscription_module/model_feed.rs",
            "cratestack-migrate/src/convert/fields.rs",
            "cratestack-studio/src/api/drift.rs",
            "cratestack-studio/src/data/db_errors.rs",
            "cratestack-studio/src/data/model_info.rs",
            "cratestack-studio/src/data/relations.rs",
        ],
    ),
    (
        "sql_column_of",
        &[
            "cratestack-macros/src/model/descriptor/unique.rs",
            "cratestack-macros/src/policy/model/term.rs",
            "cratestack-migrate/src/convert/storage.rs",
        ],
    ),
];

/// Paths no reader of a storage name may be in: they build routes, wire
/// shapes or clients, which the mapping must not move.
fn forbidden(path: &str) -> bool {
    path.starts_with("cratestack-client-")
        || path.starts_with("cratestack-mock-wiremock/")
        || path.starts_with("cratestack-macros/src/client/")
        || path == "cratestack-macros/src/axum/model/routes.rs"
        || path == "cratestack-macros/src/axum/model/prep.rs"
        || path == "cratestack-macros/src/axum/model/handler_subscribe.rs"
        || path == "cratestack-macros/src/transport/rpc/model_dispatch.rs"
}

#[test]
fn storage_names_have_only_the_pinned_readers() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let crates = crates.canonicalize().expect("crates dir");
    if !crates.join("cratestack-macros/src").is_dir() {
        return; // a packaged crate has no sibling sources to read
    }
    for (helper, pinned) in STORAGE_READERS {
        let found = readers_of(helper, &crates);
        assert_eq!(
            found, *pinned,
            "{helper}: its set of callers changed. A storage name is SQL only: if the new \
             caller builds a route, op id, client or wire field, it must not read the \
             mapping; otherwise update STORAGE_READERS"
        );
        assert!(
            !found.iter().any(|path| forbidden(path)),
            "{helper}: a route, wire or client file reads a storage name"
        );
    }
}

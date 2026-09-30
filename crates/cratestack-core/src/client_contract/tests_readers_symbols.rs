//! Who reads what `@pii` / `@sensitive` become. See `tests_readers.rs`.

/// `@pii` and `@sensitive` are dropped because their only effect is audit
/// redaction. Every reader of what they become (the descriptor's column lists
/// and the redactor) is pinned here, so redaction reaching a response or
/// error body, which would retype a field on the wire, fails the test and
/// forces `@pii` / `@sensitive` back onto the contract.
pub(super) const SYMBOL_READERS: &[(&str, &[&str])] = &[
    (
        "pii_columns",
        &[
            "cratestack-macros/src/model/descriptor.rs",
            "cratestack-macros/src/model/descriptor/columns.rs",
            "cratestack-sql/src/descriptor/mod.rs",
            "cratestack-sql/src/descriptor/model_impls.rs",
            "cratestack-sql/src/descriptor/read_source.rs",
            "cratestack-sqlx/src/audit.rs",
            "cratestack-sqlx/src/audit/redact.rs",
        ],
    ),
    (
        "sensitive_columns",
        &[
            "cratestack-macros/src/model/descriptor.rs",
            "cratestack-macros/src/model/descriptor/columns.rs",
            "cratestack-sql/src/descriptor/mod.rs",
            "cratestack-sql/src/descriptor/model_impls.rs",
            "cratestack-sql/src/descriptor/read_source.rs",
            "cratestack-sqlx/src/audit.rs",
            "cratestack-sqlx/src/audit/redact.rs",
        ],
    ),
    (
        "redact_snapshot",
        &[
            "cratestack-sqlx/src/audit.rs",
            "cratestack-sqlx/src/audit/redact.rs",
        ],
    ),
];

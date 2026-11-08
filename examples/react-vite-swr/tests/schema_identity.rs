//! Splitting `schema.cstack` into a root plus `schema/` parts (cratestack#924)
//! must not change the schema's identity. The digest below was captured from
//! the single-file schema, before the split.

use react_vite_swr_example::schema::{SCHEMA_FILES, SCHEMA_SHA256};

const PRE_SPLIT_SCHEMA_SHA256: &str =
    "02cc93609f6e33ef57205aa2bda8a1e6ea731cfe9de9468042ff673c83a8e408";

#[test]
fn splitting_the_schema_did_not_change_its_identity() {
    assert_eq!(SCHEMA_SHA256, PRE_SPLIT_SCHEMA_SHA256);
    assert_eq!(SCHEMA_FILES.len(), 4);
}

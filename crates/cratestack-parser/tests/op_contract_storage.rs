//! `@@map` / `@map` are on the drop list: they move only SQL, so no op digest
//! changes (cratestack#1375). Companion to `op_contract_dropped.rs`.

mod common;
use common::*;

#[test]
fn storage_map_moves_nothing() {
    assert_unchanged(&edit(
        "  body String\n}",
        "  body String @map(\"body_text\")\n}",
    ));
    assert_unchanged(&edit(
        "  body String\n}",
        "  body String\n\n  @@map(\"legacy_notes\")\n}",
    ));
    assert_unchanged(&edit(
        "  body String\n}",
        "  body String @pii @map(\"body_enc\")\n\n  @@map(\"legacy_notes\")\n}",
    ));
}

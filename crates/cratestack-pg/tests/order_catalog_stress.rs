//! Regression test for cratestack#256: `collect_relation_order_targets`
//! (the REST `?orderBy=`/`?sort=` match-arm generator) was exponential in
//! to-one relation-graph *connectivity*, not model count -- the same
//! shape as the codegen bug fixed for the typed builder in #253/#252.
//! `order_catalog_stress.cstack` fans each model in to its 3 most recent
//! predecessors, comfortably past the connectivity where the old walk
//! diverged (16 models took ~60s to `cargo check`; 18 did not finish in
//! 5 minutes). See the fixture's own header for the full writeup.
//!
//! Compiling this file within the normal test suite's time budget IS the
//! regression guard (mirroring `relation_stress.rs`), but unlike that
//! fixture we can also check a concrete metric directly: `allowed_sorts`
//! staying exactly the model's own top-level scalar names -- not
//! ballooning with one entry per relation-nested path -- confirms the
//! fix rather than just that compilation happened to finish in time.

use cratestack::include_server_schema;

include_server_schema!("tests/fixtures/order_catalog_stress.cstack", db = Postgres);

#[test]
fn order_catalog_stress_schema_keeps_allowed_sorts_linear() {
    let descriptor = &cratestack_schema::models::ORDER_FAN_NODE15_MODEL;
    assert_eq!(
        descriptor.allowed_sorts,
        &[
            "id",
            "name",
            "count",
            "parent12Id",
            "parent13Id",
            "parent14Id"
        ]
    );
    assert_eq!(
        descriptor.allowed_includes,
        &["parent12", "parent13", "parent14"]
    );
}

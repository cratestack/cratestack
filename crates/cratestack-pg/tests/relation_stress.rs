//! Regression test for cratestack#252: `include_server_schema!` expansion
//! was exponential in relation-graph connectivity, not linear in model
//! count. `relation_stress.cstack` chains 10 bidirectionally-linked models
//! — comfortably past the connectivity where the old recursive emitter
//! diverged (6 chained models took ~1080s / 6.4GB to `cargo check`; a real
//! 16-model production schema could not build at all on 32GB, see #252).
//!
//! Compiling this file within the normal test suite's time budget IS the
//! regression guard: if the exponential path-enumeration behaviour ever
//! returns, this fixture's build time explodes long before it reaches the
//! model count that broke production, not after.

use cratestack::include_server_schema;

include_server_schema!("tests/fixtures/relation_stress.cstack", db = Postgres);

#[test]
fn relation_stress_schema_exposes_typed_relation_path_accessors() {
    // Compiling this file at all is most of the regression guard (see
    // module doc). This assertion just confirms the generated relation
    // path API — the one that was previously emitted once per *path*
    // rather than once per *model* — is actually usable end to end, not
    // merely that `cargo check` finished.
    let _ = cratestack_schema::stress_node0::children()
        .some()
        .name()
        .contains("leaf");
    let _ = cratestack_schema::stress_node5::parent().name().eq("root");
    let _ = cratestack_schema::stress_node9::parent().name().desc();
}

//! cratestack#328 regression guards: `db = Postgres` runtime codegen is
//! unchanged (still `sqlx::PgPool`-shaped), and `db = None` runtime
//! codegen has genuinely no `PgPool`/`sqlx` anywhere — not merely an
//! `Option<PgPool>` that happens to always be `None`.
//!
//! The empty fifth argument is `query_accessors` (cratestack#867). Passing
//! `&[]` is the case that matters for these guards: a schema with no
//! `query` blocks must still generate byte-identical tokens to before the
//! construct existed, which is what the `db = Postgres` assertion below
//! pins.

use super::super::super::parse::ServerDb;
use super::build_runtime_block;

#[test]
fn postgres_runtime_block_still_takes_a_pgpool_builder() {
    let generated =
        build_runtime_block(ServerDb::Postgres, &[], &[], &[], &[], false, false).to_string();

    assert!(
        generated.contains("fn builder (pool : :: cratestack :: sqlx :: PgPool)"),
        "db = Postgres must keep its existing `Cratestack::builder(pool: PgPool)` \
         signature byte-for-byte — generated tokens were: {generated}"
    );
    assert!(generated.contains("fn pool (& self) -> & :: cratestack :: sqlx :: PgPool"));
    assert!(generated.contains("runtime : :: cratestack :: __private :: SqlxRuntime"));
}

#[test]
fn none_runtime_block_builder_takes_no_pool_parameter() {
    let generated =
        build_runtime_block(ServerDb::None, &[], &[], &[], &[], false, false).to_string();

    assert!(
        generated.contains("fn builder () -> CratestackBuilder"),
        "db = None must produce a `Cratestack::builder()` with zero parameters — \
         generated tokens were: {generated}"
    );
}

#[test]
fn none_runtime_block_never_mentions_pgpool_or_sqlx() {
    let generated =
        build_runtime_block(ServerDb::None, &[], &[], &[], &[], false, false).to_string();

    assert!(
        !generated.contains("PgPool"),
        "db = None runtime block must not reference `PgPool` at all — found it in: {generated}"
    );
    assert!(
        !generated.contains("sqlx"),
        "db = None runtime block must not reference `sqlx` at all — found it in: {generated}"
    );
    assert!(
        !generated.contains("SqlxRuntime"),
        "db = None runtime block must not reference `SqlxRuntime` at all — found it in: \
         {generated}"
    );
}

#[test]
fn none_runtime_block_cratestack_type_is_a_zero_field_marker() {
    let generated =
        build_runtime_block(ServerDb::None, &[], &[], &[], &[], false, false).to_string();

    // Not `struct Cratestack { runtime: ... }`, not `struct Cratestack {
    // pool: Option<PgPool> }` — a bare marker with no fields at all, so
    // there is no field to (mis)construct or unwrap.
    assert!(generated.contains("pub struct Cratestack ;"));
    assert!(!generated.contains("fn events"));
    assert!(!generated.contains("fn views"));
}

#[test]
fn none_runtime_block_carries_a_backplane_only_for_topics() {
    let generated =
        build_runtime_block(ServerDb::None, &[], &[], &[], &[], true, false).to_string();

    assert!(generated.contains("pub struct Cratestack { backplane :"));
    assert!(generated.contains("fn with_backplane"));
    assert!(!generated.contains("sqlx"), "{generated}");
}

#[test]
fn none_runtime_block_carries_a_revocation_registry_only_for_subscription_procedures() {
    let with = build_runtime_block(ServerDb::None, &[], &[], &[], &[], false, true).to_string();
    assert!(with.contains("stream_revocations : :: cratestack :: StreamRevocations"));
    assert!(with.contains("fn with_stream_revocations"));
    assert!(with.contains("fn stream_revocations"));
    assert!(!with.contains("backplane"), "{with}");
    assert!(!with.contains("derive (Clone , Copy)"), "{with}");

    let without = build_runtime_block(ServerDb::None, &[], &[], &[], &[], false, false).to_string();
    assert!(!without.contains("StreamRevocations"), "{without}");
    assert!(without.contains("pub struct Cratestack ;"));

    let both = build_runtime_block(ServerDb::None, &[], &[], &[], &[], true, true).to_string();
    assert!(both.contains("backplane : self . backplane"));
    assert!(both.contains("stream_revocations : self . stream_revocations"));
}

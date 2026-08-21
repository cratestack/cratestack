//! cratestack#567 — JSON `null` in a PATCH body must clear a nullable
//! column, not silently no-op. Embedded counterpart of
//! `cratestack-pg/tests/nullable_patch_clear.rs` — see that file's module
//! doc for the full mechanism/design write-up
//! (`crates/cratestack-core/src/patch.rs`,
//! `crates/cratestack-macros/src/model/struct_only.rs
//! ::struct_field_definition`).
//!
//! DB-free in the "no external dependency" sense used elsewhere in this
//! repo (in-memory sqlite, synchronous, never skips in CI) — but this file
//! still proves the fix against a REAL database write (not just the
//! deserialized shape in isolation), because `include_embedded_schema!`
//! composes `Update{Model}Input` from the exact same
//! `cratestack_macros::model::inputs` functions the server schema does
//! (see `include/server/collect/models.rs` and `include/embedded.rs`), so
//! this is independent proof the fix isn't `include_server_schema!`-only.
//!
//! cratestack#663 closes the one arity #567 missed: an untouched
//! `Required`-arity field (`name`) had no `skip_serializing_if` at all and
//! still serialized as `"name":null` — see
//! `client_update_omits_untouched_required_arity_field_on_the_wire` below,
//! and the PG counterpart's module doc for the full write-up.

use cratestack::RusqliteRuntime;
use cratestack::include_embedded_schema;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/nullable_patch_clear.cstack");

use cratestack_schema::models::PatchClearTarget;
use cratestack_schema::{
    CreatePatchClearTargetInput, PATCH_CLEAR_TARGET_MODEL, UpdatePatchClearTargetInput,
};

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&PATCH_CLEAR_TARGET_MODEL))
                .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

// ───── #1 THE DECISIVE TEST: a real write clears the column ─────────────

#[test]
fn deserialized_json_null_clears_the_column_on_a_real_write() {
    let runtime = setup();
    let delegate = ModelDelegate::<PatchClearTarget, i64>::new(&runtime, &PATCH_CLEAR_TARGET_MODEL);

    delegate
        .create(CreatePatchClearTargetInput {
            id: 1,
            name: "widget".to_owned(),
            note: Some("has a note".to_owned()),
        })
        .run()
        .expect("create");

    // Same JSON a real PATCH body would carry, decoded through the exact
    // generated `Deserialize` impl — not hand-built as `Some(None)`.
    let input: UpdatePatchClearTargetInput =
        serde_json::from_str(r#"{"name": "renamed", "note": null}"#).expect("decode patch");

    let updated = delegate.update(1).set(input).run().expect("update");
    assert_eq!(updated.name, "renamed");
    assert_eq!(
        updated.note, None,
        "an explicit JSON `null` on a nullable field must clear the column to SQL NULL"
    );

    let refetched = delegate
        .find_unique(1)
        .run()
        .expect("find_unique")
        .expect("row exists");
    assert_eq!(
        refetched.note, None,
        "the clear must persist, not just appear in the returned row"
    );
}

// ───── #2 the three states, distinguishable in one suite ────────────────

#[test]
fn absent_key_leaves_the_column_untouched() {
    let runtime = setup();
    let delegate = ModelDelegate::<PatchClearTarget, i64>::new(&runtime, &PATCH_CLEAR_TARGET_MODEL);
    delegate
        .create(CreatePatchClearTargetInput {
            id: 2,
            name: "widget".to_owned(),
            note: Some("keep me".to_owned()),
        })
        .run()
        .expect("create");

    let input: UpdatePatchClearTargetInput =
        serde_json::from_str(r#"{"name": "renamed"}"#).expect("decode patch");
    let updated = delegate.update(2).set(input).run().expect("update");

    assert_eq!(updated.name, "renamed");
    assert_eq!(
        updated.note,
        Some("keep me".to_owned()),
        "omitting the key must leave the nullable column untouched"
    );
}

#[test]
fn explicit_value_sets_the_column() {
    let runtime = setup();
    let delegate = ModelDelegate::<PatchClearTarget, i64>::new(&runtime, &PATCH_CLEAR_TARGET_MODEL);
    delegate
        .create(CreatePatchClearTargetInput {
            id: 3,
            name: "widget".to_owned(),
            note: None,
        })
        .run()
        .expect("create");

    let input: UpdatePatchClearTargetInput =
        serde_json::from_str(r#"{"note": "now set"}"#).expect("decode patch");
    let updated = delegate.update(3).set(input).run().expect("update");

    assert_eq!(updated.note, Some("now set".to_owned()));
}

// ───── #3 non-nullable field is unaffected ───────────────────────────────

#[test]
fn omitting_a_non_nullable_field_still_means_untouched() {
    let runtime = setup();
    let delegate = ModelDelegate::<PatchClearTarget, i64>::new(&runtime, &PATCH_CLEAR_TARGET_MODEL);
    delegate
        .create(CreatePatchClearTargetInput {
            id: 4,
            name: "widget".to_owned(),
            note: Some("note stays".to_owned()),
        })
        .run()
        .expect("create");

    let input: UpdatePatchClearTargetInput =
        serde_json::from_str(r#"{"note": "updated"}"#).expect("decode patch");
    let updated = delegate.update(4).set(input).run().expect("update");

    assert_eq!(
        updated.name, "widget",
        "omitted non-nullable field must be untouched"
    );
    assert_eq!(updated.note, Some("updated".to_owned()));
}

// ───── #4 the deserialized shape itself, independent of any DB ──────────

#[test]
fn deserialized_shape_distinguishes_all_three_states() {
    let absent: UpdatePatchClearTargetInput = serde_json::from_str("{}").expect("decode absent");
    assert_eq!(absent.note, None);

    let explicit_null: UpdatePatchClearTargetInput =
        serde_json::from_str(r#"{"note": null}"#).expect("decode null");
    assert_eq!(explicit_null.note, Some(None));

    let explicit_value: UpdatePatchClearTargetInput =
        serde_json::from_str(r#"{"note": "hi"}"#).expect("decode value");
    assert_eq!(explicit_value.note, Some(Some("hi".to_owned())));
}

// ───── #5 the outbound half: an untouched field never hits the wire ─────

#[test]
fn client_update_omits_untouched_nullable_field_on_the_wire() {
    let input = UpdatePatchClearTargetInput {
        name: Some("renamed".to_owned()),
        note: None,
    };
    let json = serde_json::to_string(&input).expect("serialize");
    assert!(
        !json.contains("note"),
        "an untouched nullable field must be omitted from the wire, not sent as null: {json}"
    );
}

// ───── #6 cratestack#663: an untouched `Required`-arity field is ALSO
// omitted from the wire, not just `Optional`-arity ones ────────────────────

#[test]
fn client_update_omits_untouched_required_arity_field_on_the_wire() {
    // `name` is `Required` arity (a `NOT NULL` column, single-layer
    // `Option<T>` on the patch struct — no inner "clear" state at all).
    // Before cratestack#663's fix, `struct_field_definition`'s match had no
    // arm for `TypeArity::Required` at all — it fell through to the empty
    // `else`, carrying no `skip_serializing_if` — so an untouched `name`
    // serialized as `"name":null`, the same shape #567 fixed for
    // `Optional`-arity fields. `note` here is genuinely touched (set to a
    // real value) so the request isn't vacuous.
    let input = UpdatePatchClearTargetInput {
        name: None,
        note: Some(Some("hi".to_owned())),
    };
    let json = serde_json::to_string(&input).expect("serialize update input");
    assert!(
        !json.contains("name"),
        "an untouched Required-arity field must be omitted from the wire, \
         not sent as `null`, matching Optional-arity fields: {json}"
    );
}

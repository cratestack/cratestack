//! Regression test for the projection-decoder's "missing field"
//! behaviour.
//!
//! Before this slice, the macro-emitted `decode_projected_field`
//! helper rejected any JSON object that omitted a projected field's
//! key, regardless of whether the Rust field was `Option<_>` —
//! which made adding a new optional column to a server's projection
//! a breaking change for every downstream consumer (real or mocked)
//! that hadn't yet been updated to include the field in its
//! payload. Concrete case: vaam PR #47 added
//! `Vendor.refundWindowSeconds Int?`; the existing wiremock fixture
//! omitted it; all 7 checkout tests failed silently with a
//! "missing field" decode error.
//!
//! Now: missing key on an `Optional` / `List` arity field → `None`
//! / `Vec::new()`, matching what serde would do for the same field
//! on the model struct directly. Missing key on a required field
//! is still a hard error.
//!
//! This test runs the decode path end-to-end (no database — every
//! assertion is on the `Projected::<field>()` accessor return
//! value) so the new behaviour is locked in against future
//! regressions.

use cratestack::include_server_schema;

include_server_schema!("tests/fixtures/projection_tolerance.cstack", db = Postgres);

use cratestack_schema::vendor;

#[test]
fn optional_field_missing_from_payload_decodes_to_none() {
    let selection = vendor::select().id().name().refundWindowSeconds();
    let payload = serde_json::json!({
        "id": 7,
        "name": "Acme",
        // refundWindowSeconds intentionally omitted — pre-fix, this
        // would have failed with "projected Vendor payload is
        // missing field 'refundWindowSeconds'".
    });
    let projected = selection
        .decode_one(payload)
        .expect("decoder accepts payload omitting an optional field");
    assert_eq!(projected.id().unwrap(), 7);
    assert_eq!(projected.name().unwrap(), "Acme");
    assert_eq!(
        projected.refundWindowSeconds().unwrap(),
        None,
        "missing optional field decodes as None, not an error",
    );
}

#[test]
fn optional_field_explicit_null_decodes_to_none() {
    let selection = vendor::select().id().name().refundWindowSeconds();
    let payload = serde_json::json!({
        "id": 7,
        "name": "Acme",
        "refundWindowSeconds": null,
    });
    let projected = selection.decode_one(payload).expect("decoder accepts null");
    assert_eq!(projected.refundWindowSeconds().unwrap(), None);
}

#[test]
fn optional_field_present_value_decodes_to_some() {
    let selection = vendor::select().id().name().refundWindowSeconds();
    let payload = serde_json::json!({
        "id": 7,
        "name": "Acme",
        "refundWindowSeconds": 86400,
    });
    let projected = selection.decode_one(payload).expect("decoder ok");
    assert_eq!(projected.refundWindowSeconds().unwrap(), Some(86400));
}

// Note on list arity: the macro's `decode_projected_field` helper
// has a third fallback variant (`MissingFieldFallback::EmptyArray`)
// for `TypeArity::List` fields — Codex #93 P2 flagged that a missing
// list-arity field can't share the same `null` fallback as optional
// fields, since `serde_json::from_value::<Vec<T>>(Value::Null)`
// errors. The fix is in place (see
// `crates/cratestack-macros/src/model/selection.rs` +
// `selection_module/projected.rs`), but there's no end-to-end
// regression test for it because both composers' scalar-value
// codegen (`crates/cratestack-macros/src/shared/sql.rs:128`)
// currently panics on list-arity fields with
// `"unsupported SQLx value type for this slice"`. When that
// limitation is lifted, the regression test slot is right here.

#[test]
fn required_field_missing_from_payload_still_errors() {
    // Regression guard for the strict path. Required fields must
    // still produce a "missing field" error when the JSON omits
    // them — otherwise the decoder would silently surface a
    // default-valued required field, which would mask real server
    // bugs (e.g. a route that forgot to project the primary key).
    let selection = vendor::select().id().name();
    let payload = serde_json::json!({
        // id intentionally omitted.
        "name": "Acme",
    });
    let projected = selection
        .decode_one(payload)
        .expect("Projected::from_value only checks shape, not field presence");
    let err = projected
        .id()
        .expect_err("required field must error when missing from payload");
    assert!(
        format!("{err:?}").contains("missing field 'id'"),
        "expected missing-field error, got: {err:?}",
    );
}

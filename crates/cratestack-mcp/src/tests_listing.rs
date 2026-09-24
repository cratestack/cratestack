//! Unit tests for `listing.rs`'s hints, in the crate that owns the rule
//! (cratestack#1038 decision 3). `cratestack-api`'s `mcp_tools.rs` pins the
//! same thing through a generated table; this pins it without the macro,
//! so restoring "read `idempotentHint` from `idempotent_by_default`" fails
//! this crate's own suite too.

use cratestack_core::{OpDescriptor, OpKind};
use serde_json::{Value, json};

use crate::listing::build_listing;
use crate::table::ToolDescriptor;

const fn op(op_id: &'static str, idempotent_by_default: bool) -> OpDescriptor {
    OpDescriptor {
        op_id,
        kind: OpKind::Unary,
        input_ty: "",
        output_ty: "",
        idempotent_by_default,
        rate_limited_by_default: true,
        auth_required: false,
    }
}

/// A `@no_idempotency` mutation: the only way a mutation's descriptor is
/// `idempotent_by_default`.
static OPTED_OUT: OpDescriptor = op("procedure.touch", true);
static RESERVING: OpDescriptor = op("procedure.transfer", false);
static READ: OpDescriptor = op("procedure.whoami", true);

const OBJECT: &str = r#"{"type":"object"}"#;

static TABLE: [ToolDescriptor; 3] = [
    ToolDescriptor::new("touch", None, OBJECT, None, false, &OPTED_OUT),
    ToolDescriptor::new("transfer", None, OBJECT, None, false, &RESERVING),
    ToolDescriptor::new("whoami", None, OBJECT, None, true, &READ),
];

fn hints() -> Vec<Value> {
    build_listing(&TABLE)
        .unwrap()
        .iter()
        .map(|tool| serde_json::to_value(tool).unwrap()["annotations"].clone())
        .collect()
}

#[test]
fn a_no_idempotency_mutation_never_claims_idempotent_hint_true() {
    // Guards against a vacuous pass: the case under test is a mutation
    // whose descriptor says `idempotent_by_default`.
    assert!(TABLE[0].op.idempotent_by_default && !TABLE[0].read_only);
    let hints = hints();
    assert_eq!(
        hints[0],
        json!({ "readOnlyHint": false, "idempotentHint": false })
    );
    assert_eq!(
        hints[1],
        json!({ "readOnlyHint": false, "idempotentHint": false })
    );
}

#[test]
fn a_read_carries_no_idempotent_hint() {
    assert!(TABLE[2].op.idempotent_by_default && TABLE[2].read_only);
    assert_eq!(hints()[2], json!({ "readOnlyHint": true }));
}

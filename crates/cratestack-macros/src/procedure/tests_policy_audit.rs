//! The generator's own guard against a skipped policy attribute
//! (GHSA-69g4-xvcm-vm2j, maintainer decision 3). These build the IR by
//! hand — as a parser bug, or a parser bypassed, would hand it over — so
//! they exercise the macros' check, not the parser's.

use std::collections::BTreeSet;

use cratestack_core::{Attribute, Schema};

use super::generate_procedure_module;
use crate::policy::audit_model_policies;
use crate::query::generate_query_module;

const SCHEMA: &str = r#"
auth SessionUser {
  id Int
  role String
}

model Account {
  id Int @id
  owner Int
  @@allow("all", auth() != null)
  @@deny("update", auth().role == "banned")
}

type TransferInput {
  accountId Int
  amount Int
}

type Receipt {
  ok Boolean
}

type Total {
  total Int
}

mutation procedure transfer(args: TransferInput): Receipt
  @allow(auth() != null)
  @deny(hasRole("banned"))

query totals(userId: Int): Total
  @@sql("SELECT $1::bigint AS \"total\"")
  @allow(auth() != null)
  @deny(hasRole("banned"))
"#;

pub(super) fn schema() -> Schema {
    cratestack_parser::parse_schema(SCHEMA).expect("fixture parses")
}

pub(super) fn with_raw(attributes: &mut [Attribute], index: usize, raw: &str) {
    attributes[index].raw = raw.to_owned();
}

/// The procedure with its `@deny` line replaced by `raw`.
pub(super) fn procedure_result(raw: &str) -> Result<String, String> {
    let mut schema = schema();
    with_raw(&mut schema.procedures[0].attributes, 1, raw);
    let enums = BTreeSet::new();
    let procedure = &schema.procedures[0];
    generate_procedure_module(
        procedure,
        &schema.models,
        &schema.types,
        &enums,
        schema.auth.as_ref(),
    )
    .map(|tokens| tokens.to_string())
}

pub(super) fn query_result(raw: &str) -> Result<String, String> {
    let mut schema = schema();
    with_raw(&mut schema.queries[0].attributes, 2, raw);
    let enums = BTreeSet::new();
    generate_query_module(
        &schema.queries[0],
        &schema.types,
        &enums,
        schema.auth.as_ref(),
    )
    .map(|tokens| tokens.to_string())
}

const SKIPPED: &str = "policy attribute, but not in the exact form the generator applies";
const MISCOUNTED: &str = "policy rule(s) but";

#[track_caller]
pub(super) fn assert_refused(result: Result<String, String>, needle: &str) {
    match result {
        Ok(_) => panic!("generated code although a policy would be skipped"),
        Err(message) => assert!(message.contains(needle), "{message}"),
    }
}

#[test]
fn the_canonical_spelling_generates_both_policies() {
    let tokens = procedure_result("@deny(hasRole(\"banned\"))").expect("canonical generates");
    assert_eq!(tokens.matches("ProcedurePolicy {").count(), 2, "{tokens}");
    let tokens = query_result("@deny(hasRole(\"banned\"))").expect("canonical generates");
    assert_eq!(tokens.matches("ProcedurePolicy {").count(), 2, "{tokens}");
}

#[test]
fn a_policy_attribute_the_exact_reader_skips_is_a_compile_error() {
    for raw in [
        "@deny (hasRole(\"banned\"))",
        "@deny\t(hasRole(\"banned\"))",
        "@Deny(hasRole(\"banned\"))",
        "@ deny(hasRole(\"banned\"))",
        "@deny(hasRole(\"banned\")) // banned users",
        "@deny(hasRole(\"banned\"));",
        "@deny",
        "@authorize (Account, update, args.accountId)",
        "@AUTHORIZE(Account, update, args.accountId)",
        "@authorize(Account, update, args.accountId);",
        "@de\u{200B}ny(hasRole(\"banned\"))",
    ] {
        assert_refused(procedure_result(raw), SKIPPED);
        if !raw.contains("uthorize") {
            assert_refused(query_result(raw), SKIPPED);
        }
    }
    // A query has no reader for `@authorize`, so any one there is skipped.
    assert_refused(query_result("@authorize(Account, update, userId)"), SKIPPED);
}

#[test]
fn a_policy_hidden_behind_another_attribute_is_counted() {
    for raw in [
        "@no_idempotency @deny(hasRole(\"banned\"))",
        "@deprecated @Deny(hasRole(\"banned\"))",
        "@allow(auth() != null) @deny(hasRole(\"banned\"))",
    ] {
        assert_refused(procedure_result(raw), MISCOUNTED);
    }
    assert_refused(
        query_result("@stream @deny(hasRole(\"banned\"))"),
        MISCOUNTED,
    );
}

#[test]
fn a_model_policy_the_exact_reader_skips_is_a_compile_error() {
    let slots = ["list", "read", "detail", "create", "update", "delete"];
    let audit = |raw: &str| {
        let mut model = schema().models.remove(0);
        with_raw(&mut model.attributes, 1, raw);
        audit_model_policies("model `Account`", &model, &slots)
    };
    audit("@@deny(\"update\", auth().role == \"banned\")").expect("canonical passes");
    audit("@@deny('all', auth().role == \"banned\")").expect("`all` passes");
    for raw in [
        "@@deny (\"update\", auth().role == \"banned\")",
        "@@Deny(\"update\", auth().role == \"banned\")",
        "@@deny(\"update\", auth().role == \"banned\") // c",
        "@@deny(\"update\", auth().role == \"banned\");",
        "@@deny(\"updte\", auth().role == \"banned\")",
        "@@deny(\"read,update\", auth().role == \"banned\")",
        // Invisible characters: these look exactly like `@@deny(...)`.
        "@@de\u{200B}ny(\"update\", auth().role == \"banned\")",
        "@@\u{200B}deny(\"update\", auth().role == \"banned\")",
    ] {
        let message = audit(raw).expect_err(raw);
        assert!(message.contains(SKIPPED), "{message}");
    }
    let message = audit("@@audit @@deny(\"update\", auth().role == \"banned\")").unwrap_err();
    assert!(message.contains(MISCOUNTED), "{message}");
}

/// Maintainer decision 3: an invisible character is refused even
/// where the exact reader reads the attribute: inside a string it changes
/// the value compared (`"ban\u{200B}ned"` is not `"banned"`).
#[test]
fn an_invisible_character_in_a_policy_is_a_compile_error() {
    const INVISIBLE: &str = "an invisible character";
    for raw in [
        "@deny(hasRole(\"ban\u{200B}ned\"))",
        "@deny(hasRole(\"banned\u{AD}\"))",
        "@deny(hasRole(\"ban\u{34F}ned\"))",
        "@deny(hasRole(\"ban\u{3164}ned\"))",
        "@deny(hasRole(\"ban\u{FE0F}ned\"))",
    ] {
        assert_refused(procedure_result(raw), INVISIBLE);
        assert_refused(query_result(raw), INVISIBLE);
    }
    let mut model = schema().models.remove(0);
    with_raw(
        &mut model.attributes,
        1,
        "@@deny(\"update\", auth().role == \"b\u{2060}\")",
    );
    let message = audit_model_policies("model `Account`", &model, &["update"]).unwrap_err();
    assert!(message.contains(INVISIBLE), "{message}");
}

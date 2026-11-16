use std::path::Path;

use serde_json::Value;

use super::ops_report;

pub(super) const RPC: &str = r#"transport rpc

type Ping {
  note String
  x Int
}

procedure either(args: Ping): Ping
  @allow(hasRole("a") || hasRole("b"))

procedure gated(args: Ping): Ping
  @allow(hasRole("a") && x == 1)

procedure open(args: Ping): Ping
  @allow(true)

procedure anon(args: Ping): Ping
  @allow(auth() == null)

procedure system(args: Ping): Ping
  @allow(auth().isSystem())

procedure banned(args: Ping): Ping
  @allow(auth() != null)
  @deny(hasRole("banned"))

procedure sealed(args: Ping): Ping

procedure odd(args: Ping): Ping
  @allow(hasRole("a") && frobnicate("x"))

subscription procedure feed(args: Ping): Ping
  @allow(hasRole("watcher"))

model Widget {
  id Int @id
  published Boolean
  @@allow("read", published || hasRole("reader"))
  @@allow("create", hasRole("maker"))
  @@allow("update", auth() != null)
  @@deny("update", hasRole("frozen"))
}
"#;

const REST: &str = "model Gadget {\n  id Int @id\n  @@allow(\"read\", true)\n}\n\n\
    type Ping {\n  note String\n}\n\nprocedure ping(args: Ping): Ping\n  @allow(auth() != null)\n";

pub(super) fn report(source: &str) -> Value {
    let schema = cratestack_parser::parse_schema(source).expect("schema parses");
    let text = ops_report(&schema, Path::new("dir/admin-actions.cstack"), true).unwrap();
    serde_json::from_str(&text).unwrap()
}

pub(super) fn op(doc: &Value, key: &str) -> Value {
    doc["ops"]
        .as_array()
        .unwrap()
        .iter()
        .find(|op| op["key"] == key)
        .unwrap_or_else(|| panic!("no op {key} in {doc:#}"))
        .clone()
}

#[test]
fn the_document_names_the_schema_and_transport() {
    let doc = report(RPC);
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["schema"], "admin-actions");
    assert_eq!(doc["transport"], "rpc");
}

#[test]
fn a_disjunction_of_roles_lists_both_roles_and_needs_a_principal() {
    let either = op(&report(RPC), "procedure.either");
    assert_eq!(either["kind"], "procedure");
    assert_eq!(
        either["allow"],
        serde_json::json!([r#"hasRole("a") || hasRole("b")"#])
    );
    assert_eq!(either["roles"], serde_json::json!(["a", "b"]));
    assert_eq!(either["requires_principal"], true);
    assert_eq!(either["unanalysed"], serde_json::json!([]));
}

#[test]
fn a_conjunction_with_an_input_check_still_needs_a_principal() {
    assert_eq!(
        op(&report(RPC), "procedure.gated")["requires_principal"],
        true
    );
}

#[test]
fn true_and_an_anonymous_check_do_not_need_a_principal() {
    let doc = report(RPC);
    assert_eq!(op(&doc, "procedure.open")["requires_principal"], false);
    assert_eq!(op(&doc, "procedure.anon")["requires_principal"], false);
    assert_eq!(op(&doc, "procedure.open")["roles"], serde_json::json!([]));
}

#[test]
fn a_system_caller_check_needs_a_principal() {
    assert_eq!(
        op(&report(RPC), "procedure.system")["requires_principal"],
        true
    );
}

#[test]
fn a_deny_is_listed_but_adds_no_roles() {
    let banned = op(&report(RPC), "procedure.banned");
    assert_eq!(banned["deny"], serde_json::json!([r#"hasRole("banned")"#]));
    assert_eq!(banned["roles"], serde_json::json!([]));
    assert_eq!(banned["requires_principal"], true);
}

#[test]
fn an_op_with_no_allow_is_denied_to_everyone_and_says_so() {
    let sealed = op(&report(RPC), "procedure.sealed");
    assert_eq!(sealed["allow"], serde_json::json!([]));
    assert_eq!(sealed["requires_principal"], true);
}

#[test]
fn a_rule_the_analysis_does_not_understand_is_anonymous_and_listed() {
    let odd = op(&report(RPC), "procedure.odd");
    assert_eq!(odd["requires_principal"], false);
    assert_eq!(
        odd["unanalysed"],
        serde_json::json!([r#"hasRole("a") && frobnicate("x")"#])
    );
    assert_eq!(odd["roles"], serde_json::json!(["a"]));
}

#[test]
fn a_subscription_procedure_is_its_own_kind() {
    let feed = op(&report(RPC), "procedure.feed");
    assert_eq!(feed["kind"], "subscription");
    assert_eq!(feed["roles"], serde_json::json!(["watcher"]));
}

#[test]
fn model_ops_are_listed_per_verb_with_the_rules_of_that_verb() {
    let doc = report(RPC);
    let list = op(&doc, "model.Widget.list");
    assert_eq!(list["kind"], "model.list");
    assert_eq!(list["roles"], serde_json::json!(["reader"]));
    assert_eq!(list["requires_principal"], false);
    assert_eq!(
        op(&doc, "model.Widget.get")["roles"],
        serde_json::json!(["reader"])
    );
    let create = op(&doc, "model.Widget.create");
    assert_eq!(create["roles"], serde_json::json!(["maker"]));
    assert_eq!(create["requires_principal"], true);
    let update = op(&doc, "model.Widget.update");
    assert_eq!(update["deny"], serde_json::json!([r#"hasRole("frozen")"#]));
    assert_eq!(update["roles"], serde_json::json!([]));
    let delete = op(&doc, "model.Widget.delete");
    assert_eq!(delete["allow"], serde_json::json!([]));
    assert_eq!(delete["requires_principal"], true);
}

#[test]
fn a_rest_schema_is_keyed_by_method_and_route() {
    let doc = report(REST);
    assert_eq!(doc["transport"], "rest");
    assert_eq!(op(&doc, "GET /gadgets")["requires_principal"], false);
    assert_eq!(op(&doc, "POST /$procs/ping")["requires_principal"], true);
}

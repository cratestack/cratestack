//! Review findings on cratestack#1389 (issue #1388): relation quantifiers,
//! the verb-to-slot mapping, JSON key order, ANSI in the error string and the
//! macros' attribute audit.

use std::path::Path;

use super::tests::{op, report};
use super::{json_error, ops_report};

/// The reviewer's `rel.cstack` (B1): `none` and `every` over an `auth()`
/// comparison render `NOT EXISTS (.. AND FALSE)`, true for every row.
const REL: &str = r#"
transport rpc

auth SessionUser {
  id Int
}

model Project {
  id Int @id
  name String
  memberships Membership[] @relation(fields:[id],references:[projectId])

  @@allow("list", memberships.none.userId == auth().id)
  @@allow("detail", memberships.some.userId == auth().id)
  @@allow("delete", memberships.every.userId == auth().id)
  @@allow("update", auth().id != memberships.none.userId)
  @@allow("create", memberships.some.userId == auth().id)
}

model Membership {
  id Int @id
  projectId Int
  userId Int
  project Project @relation(fields:[projectId],references:[id])

  @@allow("read", auth() != null)
}
"#;

/// The reviewer's `sub.cstack` (B2): `list` and `detail` differ.
const SUB: &str = r#"
transport rpc

model Doc {
  id Int @id
  published Boolean
  @@allow("list", hasRole("lister"))
  @@allow("detail", auth() == null)
  @@subscribe
  @@emit(created, updated, deleted)
}
"#;

fn principal(doc: &serde_json::Value, key: &str) -> bool {
    op(doc, key)["requires_principal"].as_bool().unwrap()
}

#[test]
fn none_and_every_over_an_auth_comparison_do_not_require_a_principal() {
    let doc = report(REL);
    assert!(!principal(&doc, "model.Project.list"), "none");
    assert!(!principal(&doc, "model.Project.delete"), "every");
    assert!(
        !principal(&doc, "model.Project.update"),
        "none, auth on the left"
    );
}

#[test]
fn some_over_an_auth_comparison_still_requires_a_principal() {
    let doc = report(REL);
    assert!(principal(&doc, "model.Project.get"), "detail, some");
    assert!(principal(&doc, "model.Project.create"), "create, some");
}

#[test]
fn subscribe_reads_the_detail_slot_like_get_not_the_list_slot() {
    let doc = report(SUB);
    let list = op(&doc, "model.Doc.list");
    assert_eq!(list["roles"], serde_json::json!(["lister"]));
    assert!(principal(&doc, "model.Doc.list"));
    for key in ["model.Doc.get", "model.Doc.subscribe"] {
        let row = op(&doc, key);
        assert_eq!(row["allow"], serde_json::json!(["auth() == null"]), "{key}");
        assert_eq!(row["roles"], serde_json::json!([]), "{key}");
        assert!(!principal(&doc, key), "{key}");
    }
}

/// The keys of the first JSON object that starts at or after `from` in
/// `text`, in document order, read without a map so order survives. Only the
/// object's own keys: nesting is skipped, and string contents never count.
fn first_object_keys(text: &str, from: usize) -> Vec<String> {
    let start = from + text[from..].find('{').expect("an object follows");
    let (mut depth, mut keys) = (0usize, Vec::new());
    let (mut in_string, mut escaped, mut current) = (false, false, String::new());
    let mut chars = text[start..].chars().peekable();
    while let Some(ch) = chars.next() {
        if in_string {
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => {
                    in_string = false;
                    // A key is a string at depth 1 followed by `:`.
                    while chars.next_if(|c| c.is_whitespace()).is_some() {}
                    if depth == 1 && chars.peek() == Some(&':') {
                        keys.push(std::mem::take(&mut current));
                    }
                    current.clear();
                }
                (false, c) => current.push(c),
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return keys;
                }
            }
            _ => {}
        }
    }
    panic!("unterminated object in {text}");
}

#[test]
fn the_json_keys_come_out_in_the_documented_order() {
    let schema = cratestack_parser::parse_schema(SUB).unwrap();
    let text = ops_report(&schema, Path::new("s.cstack"), true).unwrap();
    assert_eq!(
        first_object_keys(&text, 0),
        ["ok", "schema", "transport", "ops"]
    );
    let ops_at = text.find("\"ops\"").unwrap();
    assert_eq!(
        first_object_keys(&text, text[ops_at..].find('[').unwrap() + ops_at),
        [
            "key",
            "kind",
            "allow",
            "deny",
            "roles",
            "requires_principal",
            "unanalysed",
            "contract_digest",
        ],
        "every op row's keys, in the documented order"
    );
}

#[test]
fn the_error_document_has_no_terminal_colour_codes() {
    let error = anyhow::anyhow!("\u{1b}[1;31merror\u{1b}[0m: \u{1b}[34mbad\u{1b}[0m schema");
    let text = json_error(&error);
    assert!(
        !text.contains('\u{1b}') && !text.contains("\\u001b"),
        "{text}"
    );
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["ok"], false);
    assert_eq!(doc["error"], "error: bad schema");
    assert!(text.find("\"ok\"") < text.find("\"error\""));
}

#[test]
fn a_rule_the_macros_audit_would_drop_is_unanalysed_not_ignored() {
    // `@allow (x)` is a spelling the readers skip; the macros refuse to
    // generate it (GHSA-69g4-xvcm-vm2j). Hand-edited IR, as a parser bug
    // would hand it over.
    let mut schema = cratestack_parser::parse_schema(super::tests::RPC).unwrap();
    for procedure in &mut schema.procedures {
        if procedure.name == "either" {
            procedure.attributes[0].raw = "@allow (hasRole(\"a\"))".to_owned();
        }
    }
    for model in &mut schema.models {
        model.attributes[0].raw = "@@allow (\"read\", true)".to_owned();
    }
    let text = ops_report(&schema, Path::new("s.cstack"), true).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    let either = op(&doc, "procedure.either");
    assert_eq!(either["requires_principal"], false);
    assert!(
        either["unanalysed"][0]
            .as_str()
            .unwrap()
            .contains("GHSA-69g4-xvcm-vm2j")
    );
    let list = op(&doc, "model.Widget.list");
    assert_eq!(list["requires_principal"], false);
    assert!(!list["unanalysed"].as_array().unwrap().is_empty());
}

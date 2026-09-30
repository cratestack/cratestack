use super::*;

const SCHEMA: &str = "transport rpc\n\nmodel Widget {\n  id Int @id\n}\n\n\
    type Ping {\n  note String\n}\n\nprocedure ping(args: Ping): Ping\n";

fn schema(source: &str) -> Schema {
    cratestack_parser::parse_schema(source).expect("schema parses")
}

#[test]
fn digest_lists_every_op_and_the_client_contract() {
    let out = digest_report(&schema(SCHEMA), false);
    assert!(out.starts_with("client contract  "));
    for key in ["model.Widget.list", "model.Widget.delete", "procedure.ping"] {
        assert!(out.contains(key), "{out}");
    }
    assert_eq!(out.lines().count(), 1 + 6);
}

#[test]
fn json_digest_is_a_stable_document() {
    let out = digest_report(&schema(SCHEMA), true);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["client_contract"].as_str().unwrap().len(), 64);
    assert_eq!(doc["ops"]["procedure.ping"].as_str().unwrap().len(), 64);
    assert_eq!(digest_report(&schema(SCHEMA), true), out);
}

#[test]
fn a_policy_edit_leaves_the_printed_digests_alone() {
    let edited = SCHEMA.replace("model Widget {\n  id Int @id\n", "model Widget {\n  id Int @id\n\n  @@allow(\"read\", true)\n");
    assert_eq!(digest_report(&schema(SCHEMA), false), digest_report(&schema(&edited), false));
}

#[test]
fn print_shows_the_canonical_json_and_names_the_ops_on_a_miss() {
    let json = print_report(&schema(SCHEMA), "procedure.ping").unwrap();
    assert!(json.contains(r#""key":"procedure.ping""#));
    let err = print_report(&schema(SCHEMA), "procedure.nope").unwrap_err().to_string();
    assert!(err.contains("procedure.ping"), "{err}");
}

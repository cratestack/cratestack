//! Which `ModelDescriptor` slot each verb's rules land in (cratestack#1388,
//! review B2). `cratestack contract ops` picks the rules it analyses for a
//! verb from `cratestack_policy::model_verb_policy_slots`; this pins the
//! generated descriptor to that same table, so the analysis and the code
//! cannot read different slots. The descriptor is positional, so the test
//! reads `ModelDescriptor::new`'s arguments by index.

use cratestack_core::ModelVerb;
use cratestack_policy::model_verb_policy_slots;

use super::generate_model_descriptor;

/// `ModelDescriptor::new`'s policy parameters, in order, from argument 7.
const POLICY_PARAMS: [&str; 10] = [
    "read_allow_policies",
    "read_deny_policies",
    "detail_allow_policies",
    "detail_deny_policies",
    "create_allow_policies",
    "create_deny_policies",
    "update_allow_policies",
    "update_deny_policies",
    "delete_allow_policies",
    "delete_deny_policies",
];
const FIRST_POLICY_ARG: usize = 7;

const SCHEMA: &str = r#"
model Note {
  id Int @id
  @@allow("list", hasRole("list_allow"))
  @@deny("list", hasRole("list_deny"))
  @@allow("detail", hasRole("detail_allow"))
  @@deny("detail", hasRole("detail_deny"))
  @@allow("read", hasRole("read_allow"))
  @@allow("create", hasRole("create_allow"))
  @@deny("create", hasRole("create_deny"))
  @@allow("update", hasRole("update_allow"))
  @@deny("update", hasRole("update_deny"))
  @@allow("delete", hasRole("delete_allow"))
  @@deny("delete", hasRole("delete_deny"))
}
"#;

fn policy_args() -> Vec<String> {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("fixture parses");
    let code = generate_model_descriptor(
        &schema.models[0],
        &schema.models,
        &schema.types,
        &[],
        schema.auth.as_ref(),
        false,
    )
    .expect("descriptor generates")
    .to_string();
    let open = code.find("ModelDescriptor :: new (").expect("new call") + 24;
    let (mut depth, mut start, mut args) = (0usize, open, Vec::new());
    for (offset, ch) in code[open..].char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => break,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                args.push(code[start..open + offset].to_owned());
                start = open + offset + 1;
            }
            _ => {}
        }
    }
    args.split_off(FIRST_POLICY_ARG)
}

#[test]
fn each_verbs_rules_sit_in_the_slot_the_shared_table_names() {
    let args = policy_args();
    let name = |verb: ModelVerb| match verb {
        ModelVerb::List => "list",
        ModelVerb::Get | ModelVerb::Subscribe => "detail",
        ModelVerb::Create => "create",
        ModelVerb::Update => "update",
        ModelVerb::Delete => "delete",
    };
    for verb in [
        ModelVerb::List,
        ModelVerb::Get,
        ModelVerb::Subscribe,
        ModelVerb::Create,
        ModelVerb::Update,
        ModelVerb::Delete,
    ] {
        for slot in model_verb_policy_slots(verb) {
            for (field, kind) in [(slot.allow_field(), "allow"), (slot.deny_field(), "deny")] {
                let index = POLICY_PARAMS.iter().position(|p| *p == field).unwrap();
                let marker = format!("\"{}_{kind}\"", name(verb));
                assert!(
                    args[index].contains(&marker),
                    "{verb:?}: `{field}` should hold {marker}, got {}",
                    args[index]
                );
            }
        }
    }
}

#[test]
fn every_slot_holds_only_its_own_actions_and_read_fills_both_read_slots() {
    let args = policy_args();
    for (index, param) in POLICY_PARAMS.iter().enumerate() {
        let arg = &args[index];
        let own = param.split('_').next().unwrap();
        for other in ["list", "detail", "create", "update", "delete"] {
            let foreign = other != own && !(own == "read" && other == "list");
            assert_eq!(
                arg.contains(&format!("\"{other}_")),
                !foreign,
                "{param} / {other}: {arg}"
            );
        }
        let reads = arg.contains("\"read_allow\"");
        assert_eq!(
            reads,
            matches!(*param, "read_allow_policies" | "detail_allow_policies")
        );
    }
}

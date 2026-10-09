#![cfg(test)]
//! ADR 0019 D5 (PR A): a validator on a `type` field runs on a value a
//! client sent, so one on a `type` that no client input reaches is inert
//! and is refused. A client sends procedure arguments and the params of a
//! model's `@computed` field; `type`s nested in those are reached too.

use super::parse_schema;

const TAG: &str = "type Tag {\n  label String @length(min: 2)\n}\n";

#[track_caller]
fn refused(source: &str) -> (String, String) {
    match parse_schema(source) {
        Ok(_) => panic!("must be refused:\n{source}"),
        Err(error) => (error.to_string(), source[error.span()].to_owned()),
    }
}

#[track_caller]
fn accepted(source: &str) {
    parse_schema(source).unwrap_or_else(|e| panic!("refused: {e}\n{source}"));
}

#[test]
fn a_validator_on_a_return_only_type_is_refused() {
    let (message, underlined) = refused(&format!("{TAG}procedure p(): Tag\n"));
    assert!(
        message.contains("field `Tag.label` declares `@length(min: 2)`")
            && message.contains("no client input reaches `Tag`"),
        "{message}"
    );
    assert!(
        message.contains("validators run only on procedure arguments and `@computed` params"),
        "{message}"
    );
    assert_eq!(underlined, "@length(min: 2)");
    // Not used by a procedure at all.
    refused(TAG);
}

#[test]
fn a_type_used_as_an_argument_and_as_a_return_keeps_its_validators() {
    accepted(&format!("{TAG}procedure p(args: Tag): Tag\n"));
    accepted(&format!(
        "{TAG}procedure p(tags: Tag[], more: Tag?): String\n"
    ));
}

#[test]
fn a_type_nested_in_an_argument_is_reached_through_lists_and_optionals() {
    let nested = format!(
        "{TAG}type Owner {{\n  tags Tag[]\n  backup Tag?\n}}\ntype Account {{\n  owner Owner\n}}\n"
    );
    accepted(&format!("{nested}procedure p(args: Account): String\n"));
    // The same types, with the outermost only returned: all three are refused.
    let (message, _) = refused(&format!("{nested}procedure p(): Account\n"));
    assert!(message.contains("`Tag.label`"), "{message}");
}

#[test]
fn a_cycle_is_walked_once() {
    let node = "type Node {\n  label String @length(min: 1)\n  children Node[]\n}\n";
    accepted(&format!("{node}procedure p(args: Node): String\n"));
    refused(&format!("{node}procedure p(): Node\n"));
}

#[test]
fn a_model_computed_params_type_is_reached_by_the_client() {
    let params = "type Params {\n  width Int? @range(min: 1, max: 4000)\n}\n";
    accepted(&format!(
        "{params}model Photo {{\n  id Int @id\n  url String @computed(params: Params?)\n  \
         @@allow(\"read\", true)\n}}\n"
    ));
}

/// No params travel on a `type`'s `@computed` field (a procedure output has
/// no slot for them), so its params `type` is built by the server.
#[test]
fn a_type_computed_params_type_is_not_reached() {
    let params = "type Params {\n  width Int? @range(min: 1, max: 4000)\n}\n";
    let (message, _) = refused(&format!(
        "{params}type Card {{\n  url String @computed(params: Params?)\n}}\nprocedure p(): Card\n"
    ));
    assert!(message.contains("field `Params.width`"), "{message}");
}

#[test]
fn a_type_without_validators_is_never_refused() {
    accepted("type Plain {\n  note String\n}\nprocedure p(): Plain\n");
}

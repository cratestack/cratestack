//! `parse_<model>_computed_params` validates a key's value when its params
//! `type` carries validators (ADR 0019 D5), and only then. The behaviour on
//! the wire, REST and RPC, is `cratestack-pg/tests/computed_fields_router.rs`
//! and `computed_fields_rpc_model.rs`.

use super::computed::{build_parse_computed_params_fn, model_computed_fields};
use super::prep::build_prep;
use crate::validators::Validating;

const SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

type Checked {
  width Int? @range(min: 1, max: 4000)
}

type Unchecked {
  width Int?
}

model Photo {
  id Int @id
  checked String @computed(params: Checked?)
  unchecked String @computed(params: Unchecked?)

  @@allow("read", true)
}
"#;

fn parse_fn() -> String {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("fixture parses");
    let model = &schema.models[0];
    let validating = Validating::of(&schema.types, &schema.models);
    let fields = model_computed_fields(model, &validating);
    let prep = build_prep(model).expect("prep");
    build_parse_computed_params_fn(&prep, &fields).to_string()
}

#[test]
fn only_a_params_type_with_validators_is_decoded_and_validated_in_the_parser() {
    let tokens = parse_fn();
    // One `validate_at`, for `checked`, named as the client wrote it.
    assert_eq!(tokens.matches("validate_at").count(), 1, "{tokens}");
    assert!(
        tokens.contains("root . field (\"computedParams\")"),
        "{tokens}"
    );
    assert!(
        tokens.contains("computed . field (\"checked\")"),
        "{tokens}"
    );
    assert!(
        !tokens.contains("computed . field (\"unchecked\")"),
        "{tokens}"
    );
    // The decode it needs for that is the serializer's, same error text.
    assert!(
        tokens.contains("invalid computedParams for field"),
        "{tokens}"
    );
}

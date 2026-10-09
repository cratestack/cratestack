//! Codegen guard for argument validation (ADR 0019 D5): all four lifecycle
//! helpers validate the arguments' fields first, so no choice of helper
//! skips it, and the `@isolation` form validates before it takes a pooled
//! connection. The behavioural proof is `cratestack-api/tests/type_validators_*.rs`
//! (every helper, every transport) and `cratestack-pg/tests/procedure_isolation.rs`.

use quote::ToTokens;

use super::generate_procedure_module;
use crate::validators::Validating;

const SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  name String @length(min: 3)

  @@allow("read", auth() != null)
}

type Reply {
  ok Boolean
}

procedure rename(args: Account): Reply
  @allow(true)

procedure renameIsolated(args: Account): Reply
  @isolation("serializable")
  @allow(true)
"#;

fn module(name: &str) -> syn::ItemMod {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("fixture parses");
    let procedure = schema
        .procedures
        .iter()
        .find(|p| p.name == name)
        .expect("procedure");
    let validating = Validating::of(&schema.types, &schema.models);
    let tokens = generate_procedure_module(
        procedure,
        &schema.models,
        &schema.types,
        &Default::default(),
        None,
        &validating,
    )
    .expect("module generates");
    syn::parse2(tokens).expect("module parses")
}

/// The generated source of the function `name` in `module`.
fn body(module: &syn::ItemMod, name: &str) -> (syn::Visibility, String) {
    let (_, items) = module.content.as_ref().expect("inline module");
    items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == name => Some((
                function.vis.clone(),
                function.block.to_token_stream().to_string(),
            )),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no fn `{name}`"))
}

#[test]
fn each_of_the_four_helpers_validates_before_it_authorizes() {
    let module = module("rename");
    for (helper, authorizes) in [
        ("authorize", "authorize_procedure"),
        ("invoke", "authorize_procedure"),
        ("authorize_with_db", "authorize_validated_with_db"),
        ("invoke_with_db", "authorize_validated_with_db"),
    ] {
        let (_, code) = body(&module, helper);
        let validate = code
            .find("validate_fields")
            .unwrap_or_else(|| panic!("`{helper}` does not validate the arguments: {code}"));
        let authorize = code.find(authorizes).expect("authorizes");
        assert!(
            validate < authorize,
            "`{helper}` must validate before `{authorizes}`: {code}"
        );
    }
}

/// Only the private function has no validation, so no caller outside the
/// generated module can obtain an `Authorized` without validating.
#[test]
fn authorization_without_validation_is_private_to_the_module() {
    let module = module("rename");
    let (visibility, code) = body(&module, "authorize_validated_with_db");
    assert!(matches!(visibility, syn::Visibility::Inherited));
    assert!(!code.contains("validate_fields"), "{code}");
    assert!(code.contains("Authorized"), "{code}");
}

/// An invalid request must cost no pooled connection and no transaction, and
/// a retried attempt must not validate again: the isolated `invoke_with_db`
/// validates before `run_isolated` and authorizes without validating inside.
#[test]
fn an_isolated_procedure_validates_before_it_takes_a_connection() {
    let module = module("renameIsolated");
    let (_, code) = body(&module, "invoke_with_db");
    let validate = code.find("validate_fields").expect("validates");
    let transaction = code.find("run_isolated").expect("runs in a transaction");
    assert!(validate < transaction, "{code}");
    assert_eq!(code.matches("validate_fields").count(), 1, "{code}");
    assert!(code.contains("authorize_validated_with_db"), "{code}");
    assert!(!code.contains("authorize_with_db ("), "{code}");
}

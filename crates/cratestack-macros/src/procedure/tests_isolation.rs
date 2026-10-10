//! Codegen guard for `@isolation` (GHSA-r67q-4qqq-g9gm,
//! docs/design/procedure-isolation.md). The behavioural proof is
//! `cratestack-pg/tests/procedure_isolation*.rs` against real Postgres;
//! these pin the shape every dispatch site has to agree on: an `@isolation`
//! procedure's registry method takes the transaction-bound handle, its
//! module carries the level and the transaction-running `invoke_with_db`,
//! and a procedure without the attribute generates exactly what it did.

use super::{generate_procedure_module, generate_procedure_registry_method};
use crate::validators::Validating;

const SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

type Transfer {
  amount Int
}

mutation procedure settle(args: Transfer): Transfer
  @isolation("repeatable_read")

mutation procedure plain(args: Transfer): Transfer
"#;

fn procedures() -> Vec<cratestack_core::Procedure> {
    cratestack_parser::parse_schema(SCHEMA)
        .expect("fixture schema should parse and validate")
        .procedures
}

fn module_tokens(procedure: &cratestack_core::Procedure) -> String {
    let schema = cratestack_parser::parse_schema(SCHEMA).unwrap();
    generate_procedure_module(
        procedure,
        &schema.models,
        &schema.types,
        &Default::default(),
        None,
        &Validating::none(),
    )
    .unwrap()
    .to_string()
}

#[test]
fn isolated_registry_method_takes_the_transaction_bound_handle() {
    let procedures = procedures();
    let isolated = generate_procedure_registry_method(&procedures[0])
        .unwrap()
        .to_string();
    assert!(
        isolated.contains("db : & super :: IsolatedCratestack"),
        "{isolated}"
    );
    let plain = generate_procedure_registry_method(&procedures[1])
        .unwrap()
        .to_string();
    assert!(plain.contains("db : & super :: Cratestack"), "{plain}");
    assert!(!plain.contains("IsolatedCratestack"), "{plain}");
}

#[test]
fn isolated_module_runs_authorization_and_body_in_the_declared_transaction() {
    let procedures = procedures();
    let isolated = module_tokens(&procedures[0]);
    assert!(
        isolated.contains(
            "pub const ISOLATION : :: cratestack :: TransactionIsolation = :: cratestack :: \
             TransactionIsolation :: RepeatableRead ;"
        ),
        "{isolated}"
    );
    assert!(isolated.contains("run_isolated (ISOLATION ,"), "{isolated}");
    // Authorization happens inside the attempt, against the bound handle.
    // Validation does not: it ran once before the transaction began
    // (`tests_validation`), so a retried attempt does not repeat it.
    let run = isolated.find("run_isolated").unwrap();
    let authorize = isolated
        .rfind("authorize_validated_with_db (& tx_db . inner")
        .unwrap();
    assert!(authorize > run, "{isolated}");

    let plain = module_tokens(&procedures[1]);
    assert!(!plain.contains("ISOLATION"), "{plain}");
    assert!(!plain.contains("run_isolated"), "{plain}");
    assert!(plain.contains("F : FnOnce (Authorized) -> Fut"), "{plain}");
}

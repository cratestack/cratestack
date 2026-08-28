#![cfg(test)]
//! The generated Rust client's `Client` and `ProceduresClient` carry
//! hard-coded methods of their own, and a schema name can land on one of
//! them: `model Procedure` derives the accessor `procedures()`, which
//! `Client` already defines, and `procedure new` derives `new()`, which
//! `ProceduresClient` already defines. Both compiled to
//! `error[E0592]: duplicate definitions with name ...` — a rustc error
//! naming neither declaration — see
//! `crates/cratestack-parser/src/validate/client_method_collisions.rs`.

use crate::parse_schema;

const DATASOURCE: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}
"#;

fn error_for(body: &str) -> String {
    parse_schema(&format!("{DATASOURCE}{body}"))
        .expect_err("a name colliding with a built-in client method must be rejected")
        .to_string()
}

fn accept(body: &str) {
    parse_schema(&format!("{DATASOURCE}{body}"))
        .unwrap_or_else(|error| panic!("schema should parse:\n{body}\n\nerror: {error}"));
}

const PROCEDURE_MODEL: &str = r#"
model Procedure {
  id Int @id
  name String
}
"#;

#[test]
fn rejects_a_model_whose_accessor_is_the_clients_own_procedures_method() {
    let message = error_for(PROCEDURE_MODEL);
    assert!(message.contains("Procedure"), "error: {message}");
    assert!(message.contains("procedures()"), "error: {message}");
    assert!(message.contains("E0592"), "error: {message}");
    assert!(message.contains("rename the model"), "error: {message}");
}

#[test]
fn rejects_the_collision_however_the_model_name_is_spelled() {
    // The accessor is derived through `to_snake_case`, so PascalCase and
    // the already-snake spelling both normalize to `procedure`. All-caps
    // does NOT — see `accepts_an_all_caps_name_that_normalizes_elsewhere`.
    for name in ["Procedure", "procedure"] {
        let message = error_for(&format!("\nmodel {name} {{\n  id Int @id\n}}\n"));
        assert!(message.contains("procedures()"), "{name}: {message}");
    }
}

#[test]
fn rejects_a_procedure_named_new() {
    let message = error_for("\nprocedure new(id: Int): Int\n");
    assert!(message.contains("procedure `new`"), "error: {message}");
    assert!(message.contains("new()"), "error: {message}");
    assert!(message.contains("rename the procedure"), "error: {message}");
}

#[test]
fn rejects_a_snake_case_spelled_procedure_identically() {
    // `to_snake_case("New")` is `new`, so the casing does not save it.
    let message = error_for("\nprocedure New(id: Int): Int\n");
    assert!(message.contains("new()"), "error: {message}");
}

#[test]
fn accepts_an_all_caps_name_that_normalizes_elsewhere() {
    // `to_snake_case` inserts `_` before every uppercase character after
    // the first, so `PROCEDURE` becomes `p_r_o_c_e_d_u_r_e` and its
    // accessor is `p_r_o_c_e_d_u_r_es` — genuinely not a collision. This
    // test exists because the first draft of the one above asserted the
    // opposite and failed.
    accept("\nmodel PROCEDURE {\n  id Int @id\n}\n");
}

#[test]
fn accepts_model_names_that_only_look_close() {
    // `pluralize` appends, so these derive `batches`/`runtimes`/`rpcs`/
    // `news` — none of which is a built-in. This is the control that
    // keeps the check from being "reject anything vaguely similar".
    accept(
        r#"
model Batch {
  id Int @id
}

model Runtime {
  id Int @id
}

model New {
  id Int @id
}
"#,
    );
}

#[test]
fn accepts_a_procedure_named_after_a_client_method_that_is_not_on_procedures_client() {
    // `runtime`/`procedures`/`batch`/`rpc` live on `Client`, not on
    // `ProceduresClient`, so a procedure may legitimately take those
    // names. Rejecting them would be over-restriction, not safety.
    accept(
        r#"
procedure runtime(id: Int): Int
procedure procedures(id: Int): Int
procedure batch(id: Int): Int
procedure rpc(id: Int): Int
"#,
    );
}

#[test]
fn accepts_a_model_accessor_matching_a_procedure_method_name() {
    // Different types — `Client::posts()` and `ProceduresClient::posts()`
    // — so this compiles. Confirmed with a real `cargo check` before this
    // test was written; the parser must not reject it.
    accept(
        r#"
model Post {
  id Int @id
  title String
}

procedure posts(id: Int): Post
"#,
    );
}

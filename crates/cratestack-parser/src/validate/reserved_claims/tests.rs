//! The principal contract's reserved claim names in an `auth` block (ADR
//! 0021 §2): declared with the contract's type they are accepted, with
//! another type the checker says so.

use crate::parse_schema;

fn auth(fields: &str) -> String {
    format!(
        "type Tenant {{\n  id String\n}}\n\ntype Cnf {{\n  jkt String\n}}\n\nenum Role {{\n  admin\n  user\n}}\n\nauth User {{\n{fields}\n}}\n"
    )
}

#[test]
fn reserved_names_with_the_contract_types_are_accepted() {
    parse_schema(&auth(
        "  id Int\n  sub String\n  roles String[]\n  role Role?\n  tenant Tenant\n  \
         auth_time Int?\n  amr String[]\n  acr String\n  cnf Cnf?\n  key_role String\n  exp Int",
    ))
    .expect("the contract's own types must check");
    // `id` is not reserved: any type keeps today's behaviour.
    parse_schema(&auth("  id String\n  role String")).expect("String id");
    parse_schema(&auth("  cnf Json\n  sub Uuid")).expect("Json cnf, Uuid sub");
}

#[test]
fn a_reserved_name_with_another_type_is_refused() {
    for (field, declared, must) in [
        ("roles String", "String", "a list of strings"),
        ("roles Int[]", "Int[]", "a list of strings"),
        ("sub Int", "Int", "a string"),
        ("role String[]", "String[]", "a string"),
        ("auth_time String", "String", "an integer"),
        ("auth_time DateTime?", "DateTime?", "an integer"),
        ("exp BigInt", "BigInt", "an integer"),
        ("amr String", "String", "a list of strings"),
        ("acr Int", "Int", "a string"),
        ("cnf String", "String", "a map"),
        ("key_role Boolean", "Boolean", "a string"),
    ] {
        let error = parse_schema(&auth(&format!("  id Int\n  {field}")))
            .expect_err("a mistyped reserved claim must be refused");
        let name = field.split(' ').next().unwrap();
        let message = error.to_string();
        assert!(
            message.contains(&format!(
                "`{name}` on auth block `User` is declared `{declared}`"
            )) && message.contains("reserved claim of the principal contract")
                && message.contains(must),
            "{field}: {message}"
        );
    }
}

#[test]
fn a_facet_member_is_checked_on_the_type_it_names() {
    let source = "type Tenant {\n  id Int\n  slug String\n}\n\nauth User {\n  tenant Tenant\n}\n";
    let message = parse_schema(source)
        .expect_err("tenant.id must be a string")
        .to_string();
    assert!(
        message.contains("`tenant.id` on auth block `User` is declared `Int`")
            && message.contains("a string"),
        "{message}"
    );

    let source = "type Cnf {\n  jkt Bytes\n}\n\nauth User {\n  cnf Cnf\n}\n";
    let message = parse_schema(source)
        .expect_err("cnf.jkt must be a string")
        .to_string();
    assert!(
        message.contains("`cnf.jkt` on auth block `User`"),
        "{message}"
    );

    // A scalar `tenant` is no facet, so it reserves nothing.
    parse_schema("auth User {\n  tenant String\n}\n").expect("scalar tenant");
}

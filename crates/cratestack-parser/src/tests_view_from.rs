#![cfg(test)]
//! ADR 0019 D5 (PR A): `@from(Model.field)` on a view field is a documented
//! source annotation that nothing reads, so it accepted any text. It must at
//! least be the shape it documents and name a model and a field the schema
//! declares.

use super::parse_schema;

fn view_with(field: &str) -> String {
    format!(
        "model Customer {{\n  id Int @id\n  email String\n  @@allow(\"read\", true)\n}}\n\
         model Order {{\n  id Int @id\n  total Int\n  @@allow(\"read\", true)\n}}\n\
         view V from Customer, Order {{\n  id Int @id\n  f String {field}\n  \
         @@sql(\"SELECT 1\")\n}}\n"
    )
}

#[track_caller]
fn refused(field: &str) -> (String, String) {
    let source = view_with(field);
    match parse_schema(&source) {
        Ok(_) => panic!("`{field}` must be refused:\n{source}"),
        Err(error) => (error.to_string(), source[error.span()].to_owned()),
    }
}

#[test]
fn from_that_is_not_model_dot_field_is_refused() {
    for field in [
        "@from(!!!not a path)",
        "@from(Customer)",
        "@from(Customer.)",
        "@from(.email)",
        "@from(Customer.email.x)",
        "@from(\"Customer.email\")",
        "@from(Customer email)",
        "@from(1Customer.email)",
    ] {
        let (message, underlined) = refused(field);
        assert!(
            message.contains("`Model.field`") && message.contains("view field `V.f` writes"),
            "`{field}`: {message}"
        );
        assert_eq!(underlined, field);
    }
}

#[test]
fn from_naming_a_model_the_schema_lacks_is_refused() {
    let (message, underlined) = refused("@from(Nope.nothing)");
    assert!(
        message.contains("`Nope` is not a model in this schema")
            && message.contains("models: `Customer`, `Order`"),
        "{message}"
    );
    assert_eq!(underlined, "@from(Nope.nothing)");
}

#[test]
fn from_naming_a_field_the_model_lacks_is_refused() {
    let (message, _) = refused("@from(Customer.nothing)");
    assert!(
        message.contains("model `Customer` has no field `nothing`")
            && message.contains("its fields: `id`, `email`"),
        "{message}"
    );
}

#[test]
fn from_naming_a_real_column_stays_accepted() {
    for field in [
        "@from(Customer.email)",
        "@from(Order.total)",
        "@from( Customer.email )",
    ] {
        parse_schema(&view_with(field)).unwrap_or_else(|e| panic!("`{field}` refused: {e}"));
    }
}

//! Function-shaped `@default(...)` values (cratestack#1209): cratestack's
//! own `now()`, `uuid()`, `cuid()`, and functions the datasource registers.

use crate::parse_schema;

fn with_field(datasource_extra: &str, field: &str) -> String {
    format!(
        "datasource db {{\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n\
         {datasource_extra}\n}}\n\nmodel Widget {{\n  key String @id\n  {field}\n}}\n"
    )
}

fn refused(datasource_extra: &str, field: &str) -> String {
    parse_schema(&with_field(datasource_extra, field))
        .expect_err(field)
        .to_string()
}

#[test]
fn builtins_are_accepted_on_the_types_they_generate() {
    for field in [
        "id Uuid @default(uuid())",
        "ref String @default(uuid())",
        "slug Cuid @default(cuid())",
        "code String @default(cuid())",
        "createdAt DateTime @default(now())",
        "seenAt DateTime? @default(now())",
    ] {
        parse_schema(&with_field("", field)).unwrap_or_else(|e| panic!("{field}: {e}"));
    }
}

#[test]
fn builtins_on_another_type_are_refused() {
    for (field, generates) in [
        ("count Int @default(uuid())", "`Uuid` or `String`"),
        ("at String @default(now())", "`DateTime`"),
        ("tags String[] @default(cuid())", "is `String[]`"),
    ] {
        let message = refused("", field);
        assert!(message.contains(generates), "{field}: {message}");
    }
}

#[test]
fn builtins_must_be_spelled_exactly_and_bare() {
    for (field, says) in [
        ("id Uuid @default(UUID())", "spelled exactly `uuid()`"),
        ("id Uuid @default(uuid(4))", "takes no argument"),
        ("at DateTime @default(now() + 1)", "takes no argument"),
        ("at DateTime @default(now)", "write `@default(now())`"),
    ] {
        let message = refused("", field);
        assert!(message.contains(says), "{field}: {message}");
    }
}

/// The bare word is only refused where the builtin's value would fit: on
/// a `String`, `now` is an ordinary literal.
#[test]
fn a_bare_builtin_name_on_another_type_is_a_literal() {
    parse_schema(&with_field("", "label String @default(now)")).expect("a literal");
}

#[test]
fn unregistered_functions_are_refused_with_a_way_out() {
    let message = refused("", "id Uuid @default(gen_random_uuid())");
    assert!(message.contains("`uuid()` is built in"), "{message}");
    assert!(
        message.contains("functions = [\"gen_random_uuid\"]"),
        "{message}"
    );

    let message = refused("", "ref String @default(my_fn())");
    assert!(message.contains("neither one of cratestack's"), "{message}");
}

#[test]
fn registered_functions_are_accepted() {
    let registered = "  functions = [\"my_fn\", \"public.other_fn\"]";
    for field in [
        "ref String @default(my_fn())",
        "ref String @default(my_fn('x', 2))",
        "ref String @default(public.other_fn())",
    ] {
        parse_schema(&with_field(registered, field)).unwrap_or_else(|e| panic!("{field}: {e}"));
    }
}

#[test]
fn auth_inside_a_database_expression_is_refused() {
    let registered = "  functions = [\"lower\"]";
    let message = refused(registered, "owner String @default(lower(auth().email))");
    assert!(
        message.contains("`auth()` is not a database function"),
        "{message}"
    );
    assert!(refused("", "owner String @default(lower(auth().email))").contains("auth()"));
}

#[test]
fn malformed_or_reserved_registrations_are_refused() {
    for (extra, says) in [
        ("  functions = \"my_fn\"", "list of quoted function names"),
        ("  functions = [my_fn]", "`my_fn` is not one"),
        ("  functions = [\"uuid\"]", "`uuid()` is cratestack's own"),
        (
            "  functions = [\"DbGenerated\"]",
            "`dbgenerated()` is cratestack's own",
        ),
    ] {
        let message = refused(extra, "ref String");
        assert!(message.contains(says), "{extra}: {message}");
    }
}

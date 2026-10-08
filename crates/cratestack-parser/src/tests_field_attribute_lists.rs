#![cfg(test)]
//! ADR 0019 D5: a field attribute is accepted only if the field's own
//! declaration kind lists it (`validate::field_attribute_tables`), so an
//! unknown name, or a name of another kind, is an error where it used to be
//! inert. The census of committed schemas is `tests/committed_schemas.rs`.

use super::{FieldHost, field_attribute_names, parse_schema};

pub(crate) const UNION: [&str; 19] = [
    "@id",
    "@unique",
    "@default",
    "@relation",
    "@computed",
    "@readonly",
    "@server_only",
    "@version",
    "@pii",
    "@sensitive",
    "@db_enforce",
    "@email",
    "@uri",
    "@iso4217",
    "@length",
    "@range",
    "@regex",
    "@rename",
    "@from",
];

/// A one-field declaration of `host` carrying `attribute` (a whole `@x(..)`
/// text) on a field of type `ty`.
pub(crate) fn declaration(host: FieldHost, ty: &str, attribute: &str) -> String {
    match host {
        FieldHost::Model => format!("model T {{\n  id Int @id\n  f {ty} {attribute}\n}}\n"),
        FieldHost::View => format!(
            "model A {{\n  id Int @id\n}}\nview V from A {{\n  id Int @id\n  f {ty} {attribute}\n  \
             @@sql(\"SELECT id FROM a\")\n}}\n"
        ),
        FieldHost::Mixin => format!("mixin M {{\n  f {ty} {attribute}\n}}\n"),
        FieldHost::Type => format!("type T {{\n  f {ty} {attribute}\n}}\nprocedure p(): T\n"),
        FieldHost::Auth => format!("auth Ctx {{\n  id Int\n  f {ty} {attribute}\n}}\n"),
    }
}

#[track_caller]
pub(crate) fn refusal(host: FieldHost, ty: &str, attribute: &str) -> String {
    let source = declaration(host, ty, attribute);
    match parse_schema(&source) {
        Ok(_) => panic!("{attribute} on {host:?} must be refused:\n{source}"),
        Err(error) => error.to_string(),
    }
}

/// The unknown names ADR 0019 E1 and E12 show passing `check` on every kind.
#[test]
fn an_unknown_name_is_refused_on_every_kind_naming_the_kind_and_the_list() {
    for host in FieldHost::ALL {
        for attribute in [
            "@string",
            "@wire(string)",
            "@bigint",
            "@totallyBogusAttribute(x)",
            "@immutable",
        ] {
            let message = refusal(host, "String", attribute);
            let written = attribute.split('(').next().unwrap();
            let kind = format!("{} field", host.label());
            assert!(
                message.contains(&format!("unsupported attribute `{written}` on a")),
                "{host:?} {attribute}: {message}"
            );
            assert!(message.contains(&kind), "{host:?} {attribute}: {message}");
            assert!(
                !message.contains("did you mean"),
                "{host:?} {attribute}: {message}"
            );
            let names = field_attribute_names(host);
            if names.is_empty() {
                assert!(message.contains("accepts no attributes"), "{message}");
            }
            for name in names {
                assert!(
                    message.contains(&format!("`{name}`")),
                    "{host:?} {attribute} must list `{name}`: {message}"
                );
            }
        }
    }
}

/// Names the language knows, written on a kind that does not list them.
/// A few get a message of their own from a placement check that runs first.
#[test]
fn a_name_of_another_kind_is_refused() {
    let own_message = [
        (FieldHost::Type, "@server_only"),
        (FieldHost::Auth, "@server_only"),
        (FieldHost::Mixin, "@computed"),
        (FieldHost::View, "@computed"),
        (FieldHost::Auth, "@computed"),
        (FieldHost::Mixin, "@id"),
        (FieldHost::View, "@rename"),
        (FieldHost::Type, "@rename"),
        (FieldHost::Auth, "@rename"),
    ];
    for host in FieldHost::ALL {
        for name in UNION {
            if field_attribute_names(host).contains(&name) {
                continue;
            }
            let attribute = match name {
                "@default" => "@default(1)".to_owned(),
                "@relation" => "@relation(fields: [a], references: [b])".to_owned(),
                "@length" | "@range" => format!("{name}(min: 1)"),
                "@regex" => "@regex(\"a\")".to_owned(),
                "@rename" => "@rename(from = \"old\")".to_owned(),
                "@from" => "@from(A.id)".to_owned(),
                other => other.to_owned(),
            };
            let message = refusal(host, "Int", &attribute);
            if !own_message.contains(&(host, name)) {
                assert!(
                    message.contains(&format!("unsupported attribute `{name}`")),
                    "{host:?} {name}: {message}"
                );
            }
        }
    }
    // `@from` is a view field's attribute; a model field names the view-only
    // attribute as such.
    let message = refusal(FieldHost::Model, "Int", "@from(A.id)");
    assert!(
        message.contains("A model field accepts only `@id`"),
        "{message}"
    );
}

/// A near-miss of a name another kind accepts is pointed at it, and the
/// refusal says the kind does not take it.
#[test]
fn a_typo_of_a_name_the_kind_lacks_still_suggests_it() {
    for host in [FieldHost::Type, FieldHost::View, FieldHost::Auth] {
        let message = refusal(host, "String", "@raedonly");
        assert!(message.contains("did you mean `@readonly`?"), "{message}");
        assert!(message.contains("does not take it"), "{message}");
    }
    let message = refusal(FieldHost::Model, "String", "@raedonly");
    assert!(message.contains("(did you mean `@readonly`?)"), "{message}");
    assert!(!message.contains("does not take it"), "{message}");
}

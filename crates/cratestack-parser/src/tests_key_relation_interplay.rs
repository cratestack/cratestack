#![cfg(test)]
//! cratestack#1074 (`@id` exact, one `@relation`) together with
//! GHSA-69g4-xvcm-vm2j (trailing comments dropped, no-argument spellings
//! refused): one check refuses each spelling, and a comment carries no
//! attribute for either.

use super::parse_schema;

const RELATION: &str = "@relation(fields:[authorId],references:[id])";

fn post(attributes: &str) -> String {
    format!(
        "model User {{\n  id Int @id\n}}\n\nmodel Post {{\n  id Int @id\n  authorId Int\n  \
         author User {attributes}\n}}\n"
    )
}

#[test]
fn a_relation_in_a_trailing_comment_is_not_a_second_one() {
    let schema = parse_schema(&post(&format!("{RELATION} // was {RELATION}")))
        .expect("the comment is dropped before the relation count");
    let author = &schema.models[1].fields[2];
    let raws: Vec<&str> = author.attributes.iter().map(|a| a.raw.as_str()).collect();
    assert_eq!(raws, [RELATION]);
}

#[test]
fn a_second_relation_outside_the_comment_is_still_refused() {
    let error = parse_schema(&post(&format!("{RELATION} {RELATION} // note")))
        .expect_err("two `@relation`s outside the comment");
    assert!(
        error
            .to_string()
            .contains("declares `@relation` more than once"),
        "{error}"
    );
}

#[test]
fn an_id_in_a_trailing_comment_is_not_a_key() {
    let error = parse_schema("model Account {\n  id Int // @id\n}\n")
        .expect_err("a comment does not declare the key");
    assert!(
        error
            .to_string()
            .contains("model `Account` is missing an @id field"),
        "{error}"
    );
}

// `@id` takes no arguments in the field lists, which run before
// cratestack#1074's key/relation check: `@id(...)` and punctuation after
// `@id` are refused by the one shape check every closed list shares.
#[test]
fn every_misspelled_id_is_refused_by_the_one_spelling_check() {
    for (block, field) in [
        ("model Account {\n  id Int ", "\n}\n"),
        (
            "model A {\n  id Int @id\n}\nview V from A {\n  id Int ",
            " @from(A.id)\n  @@sql(\"SELECT id FROM a\")\n}\n",
        ),
    ] {
        for (raw, needle) in [
            ("@id()", "`@id` does not take arguments"),
            ("@id(sort: Desc)", "`@id` does not take arguments"),
            ("@id;", "`;` after `@id` is not part of any attribute"),
            ("@id-x", "`-x` after `@id` is not part of any attribute"),
        ] {
            let source = format!("{block}{raw}{field}");
            let error = parse_schema(&source).expect_err(raw);
            assert!(error.to_string().contains(needle), "{raw}: {error}");
            let start = source.find(raw).expect("attribute in source");
            assert_eq!(error.span(), start..start + raw.len(), "{raw}");
        }
    }
}

// A case variant of `@id` is read as `id` by the loose policy reader but is
// no key; it is not a near-miss either (two characters sit under the
// suggestion floor), so the closed list refuses it without a suggestion.
#[test]
fn a_case_variant_of_id_beside_the_key_is_refused() {
    let error = parse_schema("model Account {\n  id Int @id\n  code String @Id\n}\n")
        .expect_err("`@Id` is on no list");
    let message = error.to_string();
    assert!(
        message.contains("unsupported attribute `@Id` on a model field"),
        "{message}"
    );
    assert!(!message.contains("did you mean"), "{message}");
}

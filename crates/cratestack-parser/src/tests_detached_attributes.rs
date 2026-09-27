//! Which declaration a procedure's or query's attributes belong to
//! (GHSA-69g4-xvcm-vm2j, maintainer decision 4; `parse::attribute_run`).
//! Before, blank lines inside an attribute run were skipped, so a
//! `@deny(...)` written above the next procedure attached to the previous
//! one — and the procedure it was written for had no deny rule.

use crate::parse_schema;

const HEAD: &str = "auth SessionUser {\n  id Int\n  role String\n}\n\n\
                    type Args {\n  n Int\n}\n\n\
                    type Summary {\n  total Int\n}\n";

#[track_caller]
fn refused(body: &str, needles: &[&str]) -> String {
    let source = format!("{HEAD}\n{body}");
    let message = parse_schema(&source)
        .err()
        .unwrap_or_else(|| panic!("must be refused, but parsed:\n{body}"))
        .to_string();
    for needle in needles {
        assert!(message.contains(needle), "missing {needle:?}: {message}");
    }
    message
}

#[test]
fn a_group_above_the_next_procedure_is_refused_naming_both() {
    refused(
        "procedure previous(args: Args): Summary\n  @allow(auth() != null)\n\n\
         @deny(hasRole(\"banned\"))\nmutation procedure transfer(args: Args): Summary\n  \
         @allow(auth() != null)\n",
        &[
            "`@deny(hasRole(\"banned\"))` is not directly under a signature",
            "between procedure `previous` and procedure `transfer`",
        ],
    );
    refused(
        "query previous(n: Int): Summary\n  @@sql(\"SELECT $1::bigint AS total\")\n  \
         @allow(true)\n\n@deny(hasRole(\"banned\"))\nquery totals(n: Int): Summary\n  \
         @@sql(\"SELECT $1::bigint AS total\")\n  @allow(true)\n",
        &["between query `previous` and query `totals`"],
    );
}

#[test]
fn a_blank_line_ends_the_attribute_run() {
    for body in [
        "procedure p(args: Args): Summary\n  @allow(auth() != null)\n\n  @deny(hasRole(\"banned\"))\n",
        "procedure p(args: Args): Summary\n\n  @allow(auth() != null)\n",
        "query q(n: Int): Summary\n  @@sql(\"SELECT $1::bigint AS total\")\n  @allow(true)\n\n  \
         @deny(hasRole(\"banned\"))\n",
    ] {
        refused(body, &["is not directly under a signature", "stands after"]);
    }
}

#[test]
fn a_group_before_any_declaration_is_refused() {
    let message = format!("{HEAD}\n@deny(hasRole(\"banned\"))\nprocedure p(args: Args): Summary\n");
    let error = parse_schema(&message).expect_err("an attribute above a signature is refused");
    assert!(
        error
            .to_string()
            .contains("between type `Summary` and procedure `p`"),
        "{error}"
    );
}

#[test]
fn a_run_leading_straight_into_the_next_declaration_is_refused() {
    refused(
        "procedure previous(args: Args): Summary\n  @allow(auth() != null)\n\
         @deny(hasRole(\"banned\"))\nmutation procedure transfer(args: Args): Summary\n  \
         @allow(true)\n",
        &[
            "`@deny(hasRole(\"banned\"))` is the last attribute of procedure `previous`",
            "procedure `transfer` starts on the very next line",
        ],
    );
    refused(
        "query q(n: Int): Summary\n  @@sql(\"SELECT $1::bigint AS total\")\n  @allow(true)\n\
         model M {\n  id Int @id\n}\n",
        &["is the last attribute of query `q`, and model `M` starts"],
    );
}

/// Comment lines are not a separator. Measured before: the `@deny` written
/// above `transfer` (under its doc comment) attached to `previous`, and
/// `transfer` answered `200 OK` to a `banned` caller.
#[test]
fn a_run_leading_into_the_next_declaration_through_comments_is_refused() {
    for between in [
        "/// Transfer: banned users may not.\n",
        "// banned users may not transfer\n",
        "  // indented\n/// docs\n",
    ] {
        refused(
            &format!(
                "procedure previous(args: Args): Summary\n  @allow(auth() != null)\n\
                 @deny(hasRole(\"banned\"))\n{between}mutation procedure transfer(args: Args): \
                 Summary\n  @allow(true)\n"
            ),
            &[
                "is the last attribute of procedure `previous`",
                "procedure `transfer` follows it with only comment lines in between",
            ],
        );
    }
    refused(
        "query q(n: Int): Summary\n  @@sql(\"SELECT $1::bigint AS total\")\n  @allow(true)\n\
         @deny(hasRole(\"banned\"))\n/// M's docs\nmodel M {\n  id Int @id\n}\n",
        &["is the last attribute of query `q`, and model `M` follows it"],
    );
}

// Positive controls: the layouts the repo's schemas and docs use.
#[test]
fn attributes_directly_under_their_signature_still_parse() {
    let source = format!(
        "{HEAD}\n/// Docs for p.\nprocedure p(args: Args): Summary\n  @allow(auth() != null)\n  \
         @deny(hasRole(\"banned\"))\n\n\n// a comment between declarations\n\
         procedure bare(args: Args): Summary\nprocedure next(args: Args): Summary\n  @allow(true)\n\
         // a comment after a run, then a blank line\n\n/// Docs for q.\nquery q(n: Int): Summary\n  @@sql(\"\"\"\n    SELECT $1::bigint AS total\n\n    \
         \"\"\")\n  @allow(true)\n"
    );
    let schema = parse_schema(&source).unwrap_or_else(|error| panic!("should parse: {error}"));
    let counts = schema
        .procedures
        .iter()
        .map(|p| (p.name.as_str(), p.attributes.len()))
        .collect::<Vec<_>>();
    assert_eq!(counts, [("p", 2), ("bare", 0), ("next", 1)]);
    assert_eq!(schema.queries[0].attributes.len(), 2);
    assert_eq!(schema.queries[0].docs, ["Docs for q."]);
}

/// A model's `@@…` after its closing brace is not a procedure attribute
/// gone astray, so the message points at the model body, not at a
/// signature (the `@@sql` of a query keeps the signature message).
#[test]
fn a_block_attribute_outside_its_body_points_at_the_body() {
    let message = refused(
        "model Doc {\n  id Int @id\n}\n  @@allow(\"read\", true) // public\n",
        &[
            "`@@allow(\"read\", true)` stands after model `Doc`",
            "belongs inside the `{ … }` of the model or view",
        ],
    );
    assert!(!message.contains("signature"), "{message}");
    refused(
        "query q(n: Int): Summary\n\n  @@sql(\"SELECT $1::bigint AS total\")\n",
        &["is not directly under a signature"],
    );
}

//! Comment lines inside a procedure's or query's attribute run
//! (GHSA-69g4-xvcm-vm2j, maintainer decision 1; `parse::attribute_run`):
//! a `//` or `///` line is skipped and does not end the run, while a blank
//! line still does.

use crate::parse_schema;

const HEAD: &str = "auth SessionUser {\n  id Int\n  role String\n}\n\n\
                    type Args {\n  n Int\n}\n\n\
                    type Summary {\n  total Int\n}\n";

#[track_caller]
fn refused(body: &str, needles: &[&str]) {
    let source = format!("{HEAD}\n{body}");
    let message = parse_schema(&source)
        .err()
        .unwrap_or_else(|| panic!("must be refused, but parsed:\n{body}"))
        .to_string();
    for needle in needles {
        assert!(message.contains(needle), "missing {needle:?}: {message}");
    }
}

/// Maintainer decision: a comment line inside a run, or between the
/// signature and its first attribute, is skipped and does not end it.
#[test]
fn comment_lines_inside_an_attribute_run_are_skipped() {
    let source = format!(
        "{HEAD}\nprocedure p(args: Args): Summary\n  // who may call it\n  @allow(true)\n  \
         // banned users\n  /// (a doc comment too)\n  @deny(hasRole(\"banned\"))\n\n\
         mutation procedure m(args: Args): Summary\n  @allow(auth() != null)\n  // note\n  \
         // another\n  @no_idempotency\n\n\
         query q(n: Int): Summary\n  // the body\n  @@sql(\"\"\"\n    SELECT $1::bigint AS \
         total\n  \"\"\")\n  // readers\n  @allow(true)\n  // except banned\n  \
         @deny(hasRole(\"banned\"))\n  // a trailing comment, then a blank line\n\n\
         procedure last(args: Args): Summary\n  // only a comment, then the text ends\n"
    );
    let schema = parse_schema(&source).unwrap_or_else(|error| panic!("should parse: {error}"));
    let raws = |attributes: &[cratestack_core::Attribute]| {
        attributes.iter().map(|a| a.raw.clone()).collect::<Vec<_>>()
    };
    assert_eq!(
        raws(&schema.procedures[0].attributes),
        ["@allow(true)", "@deny(hasRole(\"banned\"))"]
    );
    assert_eq!(
        raws(&schema.procedures[1].attributes),
        ["@allow(auth() != null)", "@no_idempotency"]
    );
    assert_eq!(schema.queries[0].attributes.len(), 3);
    assert_eq!(
        schema.queries[0].attributes[2].raw,
        "@deny(hasRole(\"banned\"))"
    );
    assert!(schema.procedures[2].attributes.is_empty());
}

/// A blank line still ends the run, with or without comments around it.
#[test]
fn a_blank_line_ends_the_run_even_among_comments() {
    for body in [
        "procedure p(args: Args): Summary\n  @allow(true)\n  // banned users\n\n  \
         @deny(hasRole(\"banned\"))\n",
        "procedure p(args: Args): Summary\n  @allow(true)\n\n  // banned users\n  \
         @deny(hasRole(\"banned\"))\n",
        "procedure p(args: Args): Summary\n  // attributes follow\n\n  @allow(true)\n",
    ] {
        refused(
            body,
            &["is not directly under a signature", "after procedure `p`"],
        );
    }
}

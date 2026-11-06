//! `@authn` (ADR 0022, cratestack#1244): a requirement the generator reads
//! parses; anything it would not read is refused at parse time, with the
//! reader's own reason, so no written requirement goes unenforced.

use crate::parse_schema;
use crate::tests_procedure_policy_spelling::HEAD;

fn pay(lines: &str) -> String {
    format!(
        "{HEAD}mutation procedure pay(args: TransferInput): Receipt\n  \
         @allow(auth() != null)\n  {lines}\n"
    )
}

#[test]
fn a_requirement_parses_and_stays_a_raw_attribute() {
    let schema = parse_schema(&pay(
        r#"@authn(maxAge: 300, amr: ["hwk", "user"], acr: ["aal2"])"#,
    ))
    .unwrap_or_else(|error| panic!("should parse: {error}"));
    let attributes = &schema.procedures[0].attributes;
    let authn = cratestack_core::schema::authn_attribute(attributes).expect("kept");
    let parsed = cratestack_core::schema::parse_authn_attribute(&authn.raw).expect("reads");
    assert_eq!(parsed.max_age, Some(300));
    assert_eq!(parsed.amr, ["hwk", "user"]);
    assert_eq!(parsed.acr, ["aal2"]);
}

#[test]
fn what_the_generator_would_not_read_is_refused() {
    for (line, reason) in [
        ("@authn(max_age: 300)", "has no argument `max_age`"),
        ("@authn(maxAge: 0)", "between 1"),
        ("@authn(amr: \"hwk\")", "must be a list"),
        ("@authn()", "needs at least one of `maxAge`, `amr`, `acr`"),
        ("@authn", "takes arguments in parentheses"),
        ("@authn (maxAge: 5)", "takes arguments in parentheses"),
        ("@Authn(maxAge: 5)", "did you mean `@authn`?"),
        (
            "@authn(maxAge: 5)\n  @authn(amr: [\"hwk\"])",
            "more than one @authn attribute",
        ),
    ] {
        let error = parse_schema(&pay(line)).expect_err(line).to_string();
        assert!(error.contains(reason), "{line}: {error}");
    }
}

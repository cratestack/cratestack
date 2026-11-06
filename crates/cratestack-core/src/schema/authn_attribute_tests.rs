use super::*;
use crate::schema::{Attribute, SourceSpan};

fn attribute(raw: &str) -> Attribute {
    Attribute {
        raw: raw.to_owned(),
        span: SourceSpan {
            start: 0,
            end: raw.len(),
            line: 1,
        },
    }
}

#[test]
fn every_key_is_read() {
    let parsed =
        parse_authn_attribute(r#"@authn(maxAge: 300, amr: ["hwk", 'user'], acr: ["urn:x:aal2"])"#)
            .expect("valid");
    assert_eq!(
        parsed,
        ParsedAuthn {
            max_age: Some(300),
            amr: vec!["hwk".into(), "user".into()],
            acr: vec!["urn:x:aal2".into()],
        }
    );
}

#[test]
fn any_one_key_is_enough() {
    assert_eq!(
        parse_authn_attribute("@authn(maxAge: 60)").unwrap().max_age,
        Some(60)
    );
    assert_eq!(
        parse_authn_attribute(r#"@authn(amr: ["otp"])"#)
            .unwrap()
            .amr,
        ["otp"]
    );
    assert_eq!(
        parse_authn_attribute(r#"@authn( acr : [ "aal3" ] )"#)
            .unwrap()
            .acr,
        ["aal3"]
    );
}

#[test]
fn malformed_attributes_are_refused_with_a_reason() {
    for (raw, reason) in [
        ("@authn", "parentheses"),
        ("@authn()", "at least one"),
        ("@authn(maxAge: 0)", "between 1"),
        ("@authn(maxAge: 31536001)", "between 1"),
        ("@authn(maxAge: \"300\")", "whole number"),
        ("@authn(maxAge: 1.5)", "whole number"),
        ("@authn(maxAge: 300, maxAge: 60)", "repeats"),
        ("@authn(max_age: 300)", "no argument `max_age`"),
        ("@authn(amr: \"hwk\")", "must be a list"),
        ("@authn(amr: [])", "at least one value"),
        ("@authn(amr: [hwk])", "quoted string"),
        ("@authn(amr: [\"hwk\", \"hwk\"])", "twice"),
        ("@authn(amr: [\"h w\"])", "letters, digits"),
        ("@authn(amr: [\"\\\"x\"])", "unclosed"),
        ("@authn(acr: [\"aal2\"]", "parentheses"),
        ("@authn(amr: [\"hwk\"], )", "empty argument"),
        ("@authn(amr: [\"hwk\")", "unclosed"),
        ("@authn(maxAge)", "key: value"),
    ] {
        let error = parse_authn_attribute(raw).expect_err(raw);
        assert!(error.contains(reason), "{raw}: {error}");
    }
}

#[test]
fn the_attribute_is_found_by_its_exact_name() {
    let attributes = [
        attribute("@allow(auth() != null)"),
        attribute("@authnx(maxAge: 1)"),
        attribute("@authn(maxAge: 1)"),
    ];
    assert_eq!(
        authn_attribute(&attributes).map(|a| a.raw.as_str()),
        Some("@authn(maxAge: 1)")
    );
    assert!(authn_attribute(&attributes[..2]).is_none());
}

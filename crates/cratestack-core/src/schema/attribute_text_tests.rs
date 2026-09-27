use super::{
    attribute_starts, comment_start, group_end, loose_attribute_names, split_attributes,
    strip_comment,
};

#[test]
fn a_comment_starts_at_the_first_double_slash_outside_a_string() {
    assert_eq!(comment_start("@deny(x) // banned"), Some(9));
    // Two slashes with a string between them are not a comment.
    assert_eq!(comment_start("@deny(x) /\"s\"/ y"), None);
    assert_eq!(comment_start("@deny(x)// banned"), Some(8));
    assert_eq!(comment_start("@@sql(\"a\") // it's @x"), Some(11));
    assert_eq!(strip_comment("@deny(x)   // (see #1)"), "@deny(x)");
    for raw in [
        "@deny(x)",
        "@deny(auth().url == \"http://x\")",
        "@deny(auth().url == 'http://x')",
        "@@sql(\"SELECT '//' FROM t\")",
        "@default(\"a // b\")",
        "@@sql(\"SELECT \\\"//\\\" FROM t\")",
        // A lone `"` inside a verbatim body does not end it.
        "@@sql(\"\"\"SELECT '\"' AS q, '//' AS s\"\"\")",
        "@@sql(\"\"\"\n  SELECT 1 // not a comment\n\"\"\")",
    ] {
        assert_eq!(comment_start(raw), None, "{raw}");
        assert_eq!(strip_comment(raw), raw, "{raw}");
    }
}

#[test]
fn a_comment_after_a_verbatim_body_is_found() {
    let raw = "@@sql(\"\"\"\n  SELECT '\"' // in the body\n\"\"\") // after @ops";
    let start = comment_start(raw).expect("comment after the body");
    assert_eq!(&raw[start..], "// after @ops");
}

#[test]
fn a_new_attribute_starts_at_an_at_sign_outside_strings_and_groups() {
    assert_eq!(attribute_starts("@server_only@unique"), [12]);
    assert_eq!(attribute_starts("@@audit @@paged"), [8]);
    assert_eq!(attribute_starts("@no_idempotency @deny(x)"), [16]);
    for raw in [
        "@default(\"a@b.c\")",
        "@deny(auth().email == 'a@b')",
        "@@sql(\"\"\"SELECT tags @> '{x}' FROM t\"\"\")",
        "@@sql(\"\"\"SELECT '\"' AS q, a @> b FROM t\"\"\")",
        "@default(\":) mail@home\")",
    ] {
        assert_eq!(attribute_starts(raw), Vec::<usize>::new(), "{raw}");
    }
}

#[test]
fn attributes_split_only_where_whitespace_precedes_them() {
    assert_eq!(
        split_attributes("@no_idempotency  @deny(x)"),
        [(0, "@no_idempotency"), (17, "@deny(x)")]
    );
    assert_eq!(
        split_attributes("@@sql(\"SELECT 1\") @deny(x == \"@y\")"),
        [(0, "@@sql(\"SELECT 1\")"), (18, "@deny(x == \"@y\")")]
    );
    assert_eq!(
        split_attributes("@deny(x)@allow(y)"),
        [(0, "@deny(x)@allow(y)")]
    );
    assert_eq!(
        split_attributes("@deny(x) banned"),
        [(0, "@deny(x) banned")]
    );
}

#[test]
fn a_group_ends_at_its_own_closing_bracket() {
    assert_eq!(group_end("@deny(a(b) || c) tail", 5), Some(16));
    assert_eq!(group_end("@deny(x == \")\")", 5), Some(15));
    assert_eq!(group_end("@deny(x", 5), None);
    assert_eq!(group_end("@deny x", 5), None);
}

#[test]
fn loose_names_ignore_case_spacing_and_strings() {
    assert_eq!(loose_attribute_names("@ Deny (x)"), ["deny"]);
    assert_eq!(loose_attribute_names("@deny\u{a0}(x)"), ["deny"]);
    assert_eq!(
        loose_attribute_names("@no_idempotency @AUTHORIZE(A, read, id)"),
        ["no_idempotency", "authorize"]
    );
    assert_eq!(loose_attribute_names("@allow(x == \"@deny\")"), ["allow"]);
    assert_eq!(
        loose_attribute_names("hasRole(\"x\"))"),
        Vec::<String>::new()
    );
}

// An invisible or punctuation character inside the name must not make a
// policy read as some other, inert attribute (GHSA-69g4-xvcm-vm2j review):
// `@@de\u{200B}ny(...)` looks exactly like `@@deny(...)`.
#[test]
fn loose_names_see_through_invisible_and_punctuation_characters() {
    for raw in [
        "@@de\u{200B}ny(\"all\", x)",
        "@@\u{200B}deny(\"all\", x)",
        "@@de\u{200D}ny(\"all\", x)",
        "@@de\u{AD}ny(\"all\", x)",
        "@@deny\u{2060}(\"all\", x)",
        "@@de\u{3164}ny(\"all\", x)",
        "@@de\u{34F}ny(\"all\", x)",
        "@@de-ny(\"all\", x)",
    ] {
        assert_eq!(loose_attribute_names(raw), ["deny"], "{raw:?}");
    }
    assert_eq!(loose_attribute_names("@@index([a, b])"), ["index"]);
    assert_eq!(loose_attribute_names("@@paged"), ["paged"]);
}

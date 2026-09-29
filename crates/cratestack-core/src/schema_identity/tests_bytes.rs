use super::{normalize_attribute_text, tests::widget};

#[test]
fn canonical_bytes_of_a_one_model_schema_are_pinned() {
    let bytes = super::canonical_bytes(&widget(0));
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        concat!(
            r#"{"auth":null,"config_blocks":[],"datasource":null,"enums":[],"#,
            r#""extensions":[],"mcp":null,"mixins":[],"#,
            r#""models":[{"attributes":[],"fields":[{"attributes":["@id"],"name":"id","#,
            r#""ty":{"arity":"required","generic_args":[],"ident_args":[],"int_args":[],"#,
            r#""name":"Int"}}],"mcp":null,"name":"Widget"}],"#,
            r#""procedures":[],"queries":[],"transport":"rest","types":[],"views":[]}"#,
        )
    );
}

#[test]
fn single_quoted_literals_keep_their_whitespace() {
    assert_eq!(
        normalize_attribute_text("@@allow( x == 'ops  lead' )"),
        "@@allow(x=='ops  lead')"
    );
    assert_ne!(
        normalize_attribute_text("@@allow(x == 'ops  lead')"),
        normalize_attribute_text("@@allow(x == 'ops lead')")
    );
    assert_eq!(
        normalize_attribute_text(r"@@allow(x == 'a  \'  b' )"),
        r"@@allow(x=='a  \'  b')"
    );
}

#[test]
fn double_quoted_literals_keep_their_whitespace() {
    assert_ne!(
        normalize_attribute_text(r#"@@allow(x == "ops  lead")"#),
        normalize_attribute_text(r#"@@allow(x == "ops lead")"#)
    );
}

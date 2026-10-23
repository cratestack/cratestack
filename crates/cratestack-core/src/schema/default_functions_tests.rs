use super::{BuiltinDefault, DefaultValue, classify_default, function_call, registered_functions};
use crate::schema::default_markers::DefaultMarker;
use crate::{ConfigEntry, Datasource, SourceSpan};

fn datasource(value: &str) -> Datasource {
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    };
    Datasource {
        docs: Vec::new(),
        name: "db".to_owned(),
        name_span: span,
        entries: vec![
            ConfigEntry {
                key: "provider".to_owned(),
                value: "\"postgresql\"".to_owned(),
            },
            ConfigEntry {
                key: "functions".to_owned(),
                value: value.to_owned(),
            },
        ],
        span,
    }
}

#[test]
fn calls_split_into_name_args_and_trailing_text() {
    let call = function_call(" public.my_fn ('a', f(1)) || 'x'").expect("a call");
    assert_eq!(call.name, "public.my_fn");
    assert_eq!(call.args, "'a', f(1)");
    assert_eq!(call.trailing, " || 'x'");
    assert!(call.closed && !call.is_bare());
    assert!(function_call("now()").expect("a call").is_bare());
    assert!(!function_call("now(").expect("a call").closed);
    for not_a_call in ["'now()'", "42", "now", "1fn()", ""] {
        assert_eq!(function_call(not_a_call), None, "{not_a_call}");
    }
}

#[test]
fn builtins_classify_in_any_case_and_keep_their_spelling() {
    for (inner, builtin) in [
        ("now()", BuiltinDefault::Now),
        ("UUID()", BuiltinDefault::Uuid),
        ("cuid(1)", BuiltinDefault::Cuid),
    ] {
        match classify_default(inner) {
            DefaultValue::Builtin(found, call) => {
                assert_eq!(found, builtin, "{inner}");
                assert!(call.name.eq_ignore_ascii_case(builtin.name()), "{inner}");
            }
            other => panic!("{inner}: {other:?}"),
        }
    }
}

#[test]
fn other_defaults_classify_by_shape() {
    assert!(matches!(
        classify_default("dbgenerated()"),
        DefaultValue::Marker(call) if call.marker == DefaultMarker::DbGenerated
    ));
    assert!(matches!(
        classify_default("autoincrement"),
        DefaultValue::Marker(call) if call.marker == DefaultMarker::Autoincrement
    ));
    assert_eq!(classify_default("auth().id"), DefaultValue::Auth);
    assert!(matches!(
        classify_default("gen_random_uuid()"),
        DefaultValue::Function(call) if call.name == "gen_random_uuid"
    ));
    assert!(matches!(
        classify_default("lower(auth().email)"),
        DefaultValue::Function(call) if call.name == "lower"
    ));
    for literal in ["'pending'", "0", "true", "ACTIVE", "now"] {
        assert_eq!(
            classify_default(literal),
            DefaultValue::Literal,
            "{literal}"
        );
    }
}

#[test]
fn registered_functions_read_the_datasource_list() {
    let listed = datasource(r#"["my_fn", "public.other_fn"]"#);
    assert_eq!(
        registered_functions(Some(&listed)),
        Ok(vec!["my_fn", "public.other_fn"])
    );
    assert_eq!(
        registered_functions(Some(&datasource("[]"))),
        Ok(Vec::new())
    );
    assert_eq!(registered_functions(None), Ok(Vec::new()));
}

#[test]
fn malformed_function_lists_are_errors() {
    for value in [
        r#""my_fn""#,
        "[my_fn]",
        r#"["my fn"]"#,
        r#"["my_fn()"]"#,
        r#"["public."]"#,
        r#"["1fn"]"#,
    ] {
        assert!(
            registered_functions(Some(&datasource(value))).is_err(),
            "{value}"
        );
    }
}

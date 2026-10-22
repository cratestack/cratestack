use super::{DefaultMarker, default_marker, marker_call};

#[test]
fn bare_markers_are_recognised() {
    assert_eq!(
        default_marker("dbgenerated()"),
        Some(DefaultMarker::DbGenerated)
    );
    assert_eq!(
        default_marker("autoincrement()"),
        Some(DefaultMarker::Autoincrement)
    );
}

#[test]
fn whitespace_around_the_name_and_paren_is_ignored() {
    // `attribute_text::split_attributes` trims only the outer edges, so the
    // space between name and `(` survives into `attribute.raw` and must not
    // defeat the match (cratestack#1128).
    assert_eq!(
        default_marker("dbgenerated ()"),
        Some(DefaultMarker::DbGenerated)
    );
    assert_eq!(
        default_marker("autoincrement ()"),
        Some(DefaultMarker::Autoincrement)
    );
    assert_eq!(
        default_marker("dbgenerated\t( )"),
        Some(DefaultMarker::DbGenerated)
    );
    assert_eq!(
        default_marker("  autoincrement (  )"),
        Some(DefaultMarker::Autoincrement)
    );
}

#[test]
fn markers_with_arguments_are_still_recognised() {
    // The parser refuses `dbgenerated(<args>)` separately; the matcher only
    // needs to recognise the call so that layer can decide.
    assert_eq!(
        default_marker("dbgenerated (\"x\")"),
        Some(DefaultMarker::DbGenerated)
    );
    assert_eq!(
        default_marker("autoincrement (1)"),
        Some(DefaultMarker::Autoincrement)
    );
}

#[test]
fn non_markers_are_none() {
    assert_eq!(default_marker("now()"), None);
    assert_eq!(default_marker("cs_cuid()"), None);
    assert_eq!(default_marker("'pending'"), None);
    assert_eq!(default_marker("\"pending\""), None);
    assert_eq!(default_marker("pending"), None);
    assert_eq!(default_marker(""), None);
}

#[test]
fn different_spelling_or_case_is_not_a_marker() {
    assert_eq!(default_marker("db_generated()"), None);
    assert_eq!(default_marker("AutoIncrement()"), None);
    assert_eq!(default_marker("auto_increment()"), None);
}

#[test]
fn missing_open_paren_is_not_a_call() {
    assert_eq!(default_marker("autoincrement"), None);
    assert_eq!(default_marker("dbgenerated x"), None);
}

#[test]
fn a_bare_call_is_the_whole_expression() {
    for inner in ["dbgenerated()", "dbgenerated ( )", "  autoincrement()  "] {
        let call = marker_call(inner).expect("a marker");
        assert!(call.is_bare(), "{inner}");
    }
}

#[test]
fn text_after_the_call_is_trailing_not_bare() {
    // Still classified as the marker, so an emitter never writes the text
    // as SQL; the parser refuses it because the call is not bare.
    for (inner, trailing) in [
        ("dbgenerated() || 'x'", " || 'x'"),
        ("dbgenerated()::text", "::text"),
        ("dbgenerated() + 1", " + 1"),
    ] {
        let call = marker_call(inner).expect("a marker");
        assert_eq!(call.marker, DefaultMarker::DbGenerated);
        assert_eq!(call.args, "");
        assert_eq!(call.trailing, trailing);
        assert!(!call.is_bare(), "{inner}");
    }
}

#[test]
fn arguments_balance_nested_parens() {
    let call = marker_call("dbgenerated(f(1)) || g()").expect("a marker");
    assert_eq!(call.args, "f(1)");
    assert_eq!(call.trailing, " || g()");
    assert!(call.closed);
}

#[test]
fn an_unclosed_call_is_not_bare() {
    let call = marker_call("dbgenerated(").expect("a marker");
    assert!(!call.closed);
    assert!(!call.is_bare());
}

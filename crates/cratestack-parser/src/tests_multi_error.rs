//! Multi-error diagnostics (`parse_schema_diagnostics`).
//!
//! The point of these is the *count* and the *ordering relationship* to the
//! single-error entry point, not the message text — the individual rules are
//! covered by the per-rule suites elsewhere in this crate.

use crate::{parse_schema_diagnostics, parse_schema_named};

const THREE_UNKNOWN_TYPES: &str = r#"datasource db {
  provider = "postgresql"
}

model User {
  id Int @id
  role Rolle
}

model Post {
  id Int @id
  status Statuss
}

model Comment {
  id Int @id
  kind Kindd
}
"#;

fn diagnostics(source: &str) -> Vec<String> {
    parse_schema_diagnostics("t.cstack", source)
        .1
        .into_iter()
        .map(|error| error.message().to_owned())
        .collect()
}

/// The case the feature exists for: three models each naming a type that does
/// not exist. Reporting one meant three save-and-retry rounds.
#[test]
fn independent_declaration_errors_are_all_reported() {
    let messages = diagnostics(THREE_UNKNOWN_TYPES);

    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].contains("Rolle"));
    assert!(messages[1].contains("Statuss"));
    assert!(messages[2].contains("Kindd"));
}

/// The anti-drift property. Both entry points run one set of checks in one
/// order, so the collected head must be exactly what the fail-fast path
/// returns. If these ever disagree, two validation paths have appeared.
#[test]
fn the_first_collected_error_is_the_one_the_fail_fast_path_returns() {
    let expected =
        parse_schema_named("t.cstack", THREE_UNKNOWN_TYPES).expect_err("fixture should be invalid");
    let messages = diagnostics(THREE_UNKNOWN_TYPES);

    assert_eq!(
        messages.first().map(String::as_str),
        Some(expected.message())
    );
}

/// Parsing has no recovery: after a syntax error the rest of the file is
/// unparsed, not valid, so there is no honest second error to report.
#[test]
fn a_syntax_error_yields_exactly_one_diagnostic() {
    let messages = diagnostics("mode User {\n  id Int @id\n}\n");

    assert_eq!(messages.len(), 1, "{messages:?}");
}

/// Stage gating, at the stage-2 → stage-3 boundary specifically.
///
/// An unsupported `provider` is a stage-2 problem. The per-declaration stage
/// must not run on top of it: several validators document that they assume
/// earlier ones passed, and a cascade of errors pointing at the wrong places is
/// worse than one real error.
///
/// The fixture deliberately uses a *datasource* failure rather than a duplicate
/// name — a duplicate name is caught in stage 1, whose hard return would make
/// this test pass even with the stage-2 gate removed.
#[test]
fn a_stage_two_failure_suppresses_the_per_declaration_stage() {
    let source = r#"datasource db {
  provider = "mysql"
}

model User {
  id Int @id
  role Rolle
}
"#;
    let messages = diagnostics(source);

    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unsupported datasource provider"));
    assert!(
        !messages[0].contains("Rolle"),
        "the per-declaration stage must not have run",
    );
}

/// Stage 1 is a hard stop for a different reason: it produces the name set
/// every later check is measured against, so there is nothing to continue with.
#[test]
fn a_stage_one_failure_stops_everything() {
    let source = r#"datasource db {
  provider = "postgresql"
}

model User {
  id Int @id
  role Rolle
}

model User {
  id Int @id
}
"#;
    let messages = diagnostics(source);

    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages.iter().all(|message| !message.contains("Rolle")));
}

#[test]
fn a_valid_schema_reports_nothing_and_returns_the_schema() {
    let source = r#"datasource db {
  provider = "postgresql"
}

model User {
  id Int @id
}
"#;
    let (schema, errors) = parse_schema_diagnostics("t.cstack", source);

    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(schema.expect("schema should parse").models.len(), 1);
}

/// Errors carry their own spans, so a client can place three squiggles rather
/// than one. A collector that lost spans would still "report three errors" and
/// be useless in an editor.
#[test]
fn each_collected_error_keeps_its_own_position() {
    let (_, errors) = parse_schema_diagnostics("t.cstack", THREE_UNKNOWN_TYPES);
    let lines = errors.iter().map(|error| error.line()).collect::<Vec<_>>();

    assert_eq!(lines.len(), 3);
    assert!(
        lines.windows(2).all(|pair| pair[0] < pair[1]),
        "expected three distinct, increasing positions, got {lines:?}",
    );
}

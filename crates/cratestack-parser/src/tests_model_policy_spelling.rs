//! Model- and view-level `@@allow` / `@@deny` the generator did not read
//! (GHSA-69g4-xvcm-vm2j). Measured on 0.12.0: every row of `REFUSED`
//! passed `cratestack check` and generated a model descriptor with zero
//! deny rules (`read_deny_policies.len() == 0`), where the canonical
//! spelling generates one.

use crate::parse_schema;

const DENY: &str = "auth().role == \"banned\"";

fn note_with(line: &str) -> String {
    format!(
        "auth SessionUser {{\n  id Int\n  role String\n}}\n\n\
         model Note {{\n  id Int @id\n  body String\n  @@allow(\"all\", auth() != null)\n  \
         {line}\n}}\n"
    )
}

fn view_with(line: &str) -> String {
    format!(
        "{}\nview NoteView from Note {{\n  id Int @id @from(Note.id)\n  \
         @@server_sql(\"SELECT id FROM notes\")\n  @@allow(\"read\", true)\n  {line}\n}}\n",
        note_with("")
    )
}

#[test]
fn every_model_spelling_the_generator_skipped_is_refused() {
    for (line, needle) in [
        (
            format!("@@deny (\"read\", {DENY})"),
            "`@@deny` must be followed directly by `(`",
        ),
        (
            format!("@@Deny(\"read\", {DENY})"),
            "this is not spelled `@@deny`",
        ),
        (
            format!("@@DENY(\"read\", {DENY})"),
            "this is not spelled `@@deny`",
        ),
        (
            format!("@@ deny(\"read\", {DENY})"),
            "this is not spelled `@@deny`",
        ),
        (
            format!("@@deyn(\"read\", {DENY})"),
            "this is not spelled `@@deny`",
        ),
        (
            format!("@@alow(\"read\", {DENY})"),
            "this is not spelled `@@allow`",
        ),
        (
            format!("@@deny(\"read\", {DENY});"),
            "`;` follows the closing `)`",
        ),
        (
            format!("@@deny(\"read\", {DENY}) banned"),
            "`banned` follows the closing `)`",
        ),
        (
            format!("@@deny(\"raed\", {DENY})"),
            "`raed` is not an action here",
        ),
        (
            format!("@@deny(\"read,update\", {DENY})"),
            "`read,update` is not an action here",
        ),
        (
            format!("@@deny({DENY})"),
            "its first argument must be a quoted action",
        ),
        (
            "@@deny(\"read\")".to_owned(),
            "it has no expression after the action",
        ),
        (
            format!("@@deny(\"read\", {DENY}"),
            "its argument list is never closed",
        ),
    ] {
        let message = parse_schema(&note_with(&line))
            .err()
            .unwrap_or_else(|| panic!("`{line}` must be refused, but the schema parsed"))
            .to_string();
        assert!(message.contains(needle), "`{line}`: {message}");
        assert!(message.contains("model `Note`"), "`{line}`: {message}");
    }
}

// A view builds only its `read` slot, so a deny naming `list`, `detail`
// or a write action was never applied to it.
#[test]
fn a_view_deny_on_an_action_the_view_never_checks_is_refused() {
    for action in ["list", "detail", "update"] {
        let line = format!("@@deny(\"{action}\", {DENY})");
        let message = parse_schema(&view_with(&line))
            .err()
            .unwrap_or_else(|| panic!("`{line}` must be refused on a view"))
            .to_string();
        assert!(
            message.contains(&format!("`{action}` is not an action here")),
            "{message}"
        );
    }
    for line in [
        format!("@@deny(\"read\", {DENY})"),
        format!("@@deny('all', {DENY})"),
    ] {
        parse_schema(&view_with(&line))
            .unwrap_or_else(|error| panic!("`{line}` should parse on a view: {error}"));
    }
}

// Measured before: each of these parsed as an unknown, inert `@@` attribute
// and the model and view got no deny rule, though the first four look
// exactly like `@@deny(...)` in an editor.
#[test]
fn a_deny_with_an_invisible_or_punctuation_character_in_its_name_is_refused() {
    for name in [
        "@@de\u{200B}ny",
        "@@\u{200B}deny",
        "@@de\u{200D}ny",
        "@@de\u{AD}ny",
        "@@de-ny",
    ] {
        let line = format!("{name}(\"all\", {DENY})");
        for source in [note_with(&line), view_with(&line)] {
            let message = parse_schema(&source)
                .err()
                .unwrap_or_else(|| panic!("`{line:?}` must be refused, but the schema parsed"))
                .to_string();
            // The invisible ones are refused as invisible characters before
            // the name is read (maintainer decision 3); `-` is visible.
            let needle = if name.contains('-') {
                "this is not spelled `@@deny`"
            } else {
                "an invisible character"
            };
            assert!(message.contains(needle), "{message}");
        }
    }
}

// Positive controls: the spellings the repo's schemas use.
#[test]
fn every_legitimate_model_policy_still_parses() {
    for line in [
        format!("@@deny(\"read\", {DENY})"),
        format!("@@deny('update', {DENY})"),
        "@@allow(\"create\", auth() != null && auth().role == \"admin\")".to_owned(),
        "@@allow(\"read\", body == \"a, b\")".to_owned(),
        "@@allow(\"list\", auth().email == 'ops@x.io')".to_owned(),
        format!("@@deny(\"all\", {DENY}) // nobody banned"),
        "@@allow(\"detail\", auth().url == \"http://x\")".to_owned(),
    ] {
        parse_schema(&note_with(&line))
            .unwrap_or_else(|error| panic!("`{line}` should parse: {error}"));
    }
}

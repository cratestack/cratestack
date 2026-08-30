use super::*;

fn not_null_promotion() -> BlockingReason {
    BlockingReason {
        table: "products".to_owned(),
        column: Some("version".to_owned()),
        cause: "column becomes NOT NULL; existing NULL rows would violate it".to_owned(),
        remedy: Some("UPDATE products SET version = <value> WHERE version IS NULL;".to_owned()),
    }
}

fn added_required_column() -> BlockingReason {
    BlockingReason {
        table: "orders".to_owned(),
        column: Some("total".to_owned()),
        cause: "new required column with no default; existing rows have no value for it".to_owned(),
        remedy: None,
    }
}

/// The contract with `cratestack_service::migrations::is_effectively_blank`:
/// an untouched scaffold must read as "no pre-script", so it costs no
/// round-trip and — critically — does not change the migration's
/// checksum. If a bare statement ever leaks into the template, every
/// scaffolded migration silently acquires a different checksum.
#[test]
fn scaffold_is_comment_only() {
    let scaffold = scaffold(&[not_null_promotion(), added_required_column()]);
    for line in scaffold.lines() {
        let line = line.trim();
        assert!(
            line.is_empty() || line.starts_with("--"),
            "scaffold must contain no executable SQL, found: {line}"
        );
    }
}

#[test]
fn scaffold_states_that_it_runs_in_the_same_transaction_as_up() {
    let scaffold = scaffold(&[not_null_promotion()]);
    assert!(scaffold.contains("SAME transaction"), "{scaffold}");
    assert!(scaffold.contains("up.sql"), "{scaffold}");
}

/// cratestack#843's core defect, as a test: the warning must name a
/// mechanism that exists. `up.pre.sql` now does — it is scaffolded by
/// `migrate diff` and executed by `apply_pending` — so naming it is
/// correct here in a way it was not before.
#[test]
fn up_warning_points_at_the_scaffolded_file() {
    let warning = up_warning(&[not_null_promotion()]);
    assert!(warning.contains("up.pre.sql"), "{warning}");
    assert!(warning.contains("same transaction"), "{warning}");
}

/// The second half of #843: the warning described the wrong operation.
/// A NOT NULL promotion adds no column, so the old text ("a required
/// column was added without a default") was simply false for the
/// reported case.
#[test]
fn up_warning_describes_the_actual_blocking_operation() {
    let warning = up_warning(&[not_null_promotion()]);
    assert!(warning.contains("products.version"), "{warning}");
    assert!(warning.contains("NOT NULL"), "{warning}");
    assert!(
        !warning.contains("required column was added"),
        "warning still describes an ADD COLUMN that did not happen: {warning}"
    );
}

#[test]
fn both_outputs_list_every_blocking_op() {
    let reasons = [not_null_promotion(), added_required_column()];
    for text in [scaffold(&reasons), up_warning(&reasons)] {
        assert!(text.contains("products.version"), "{text}");
        assert!(text.contains("orders.total"), "{text}");
    }
}

#[test]
fn remedy_template_is_rendered_for_a_backfillable_op() {
    let scaffold = scaffold(&[not_null_promotion()]);
    assert!(
        scaffold.contains("UPDATE products SET version = <value> WHERE version IS NULL;"),
        "{scaffold}"
    );
}

/// The failure this whole change exists to prevent, in miniature: never
/// hand the operator a remedy that cannot work. A column added by
/// `up.sql` does not exist while `up.pre.sql` runs, so no `UPDATE`
/// against it belongs in the template.
#[test]
fn no_remedy_template_when_a_pre_script_cannot_help() {
    let scaffold = scaffold(&[added_required_column()]);
    assert!(
        scaffold.contains("No pre-script can fix this one"),
        "{scaffold}"
    );
    assert!(
        !scaffold.contains("UPDATE orders SET total"),
        "scaffold offers an UPDATE against a column that does not exist yet: {scaffold}"
    );
}

#[test]
fn scaffold_warns_that_an_empty_database_hides_the_problem() {
    let scaffold = scaffold(&[not_null_promotion()]);
    assert!(scaffold.contains("empty table"), "{scaffold}");
}

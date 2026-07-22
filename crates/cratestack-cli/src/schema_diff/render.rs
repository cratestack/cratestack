use super::{Change, SchemaDiff, Severity};

pub(crate) fn render_human(diff: &SchemaDiff, old_label: &str, new_label: &str) -> String {
    let mut out = format!("schema diff: {old_label} -> {new_label}\n");

    if diff.changes.is_empty() {
        out.push_str("no structural changes detected\n");
        return out;
    }

    for severity in [Severity::Breaking, Severity::Additive, Severity::Internal] {
        let group: Vec<&Change> = diff
            .changes
            .iter()
            .filter(|change| change.severity == severity)
            .collect();
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{} ({}):\n", severity.label(), group.len()));
        for change in group {
            out.push_str(&format!("  - {}\n", change.message));
        }
    }

    let (breaking, additive, internal) = diff.counts();
    out.push_str(&format!(
        "\n{} change(s): {breaking} breaking, {additive} additive, {internal} internal-only\n",
        diff.changes.len()
    ));
    out
}

pub(crate) fn render_json(diff: &SchemaDiff) -> serde_json::Value {
    let (breaking, additive, internal) = diff.counts();
    serde_json::json!({
        "changes": diff.changes,
        "summary": {
            "breaking": breaking,
            "additive": additive,
            "internal": internal,
        },
    })
}

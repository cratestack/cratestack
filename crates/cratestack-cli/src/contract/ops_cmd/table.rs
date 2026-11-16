//! The human-readable table of `contract ops`.

use std::borrow::Cow;

use super::OpRow;

pub(super) fn render(rows: &[OpRow]) -> String {
    let mut lines: Vec<[Cow<'_, str>; 5]> = vec![[
        "OP".into(),
        "KIND".into(),
        "PRINCIPAL".into(),
        "ROLES".into(),
        "NOTE".into(),
    ]];
    for row in rows {
        lines.push([
            row.key.as_str().into(),
            row.kind.as_str().into(),
            if row.requires_principal {
                "required"
            } else {
                "no"
            }
            .into(),
            row.roles.join(",").into(),
            note(row).into(),
        ]);
    }
    let widths: [usize; 5] = std::array::from_fn(|column| {
        lines
            .iter()
            .map(|line| line[column].len())
            .max()
            .unwrap_or(0)
    });
    let mut out = String::new();
    for line in &lines {
        let cells: Vec<String> = line
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        out.push_str(cells.join("  ").trim_end());
        out.push('\n');
    }
    out
}

fn note(row: &OpRow) -> String {
    if !row.unanalysed.is_empty() {
        format!("unanalysed: {}", row.unanalysed.join("; "))
    } else if row.allow.is_empty() {
        "no allow rule: denied for every caller".to_owned()
    } else {
        String::new()
    }
}

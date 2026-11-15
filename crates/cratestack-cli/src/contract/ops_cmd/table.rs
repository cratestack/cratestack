//! The human-readable table of `contract ops`.

use super::OpRow;

pub(super) fn render(rows: &[OpRow]) -> String {
    let mut lines = vec![[
        "OP".to_owned(),
        "KIND".to_owned(),
        "PRINCIPAL".to_owned(),
        "ROLES".to_owned(),
        "NOTE".to_owned(),
    ]];
    for row in rows {
        lines.push([
            row.key.clone(),
            row.kind.clone(),
            if row.requires_principal {
                "required"
            } else {
                "no"
            }
            .to_owned(),
            row.roles.join(","),
            note(row),
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

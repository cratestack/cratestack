//! Human-readable drift report for `cratestack migrate baseline`
//! (issue #205, design doc §5.4). No new comparison logic — this
//! just groups `diff_projections`'s `Op` list by table/view and
//! renders each op's existing `Destructiveness`, so lossy/blocking
//! drift reads louder than a merely-missing index.

use std::collections::BTreeMap;

use cratestack_migrate::introspect::postgres::UnmappedColumn;
use cratestack_migrate::ir::{Destructiveness, Op};

/// Render the full drift report: grouped ops, then a separate
/// "unmapped columns" section — real columns the comparison above
/// couldn't see at all (design doc §5.2's "unmapped → reported drift,
/// never guessed" rule), surfaced loudly rather than silently
/// dropped.
pub(super) fn render(ops: &[Op], unmapped: &[UnmappedColumn]) -> String {
    let mut out = String::new();

    if ops.is_empty() {
        out.push_str("no drift: the live database matches the schema exactly\n");
    } else {
        out.push_str(&render_ops(ops));
    }

    if !unmapped.is_empty() {
        out.push_str(&render_unmapped(unmapped));
    }

    out
}

fn render_ops(ops: &[Op]) -> String {
    let mut grouped: BTreeMap<&str, Vec<&Op>> = BTreeMap::new();
    for op in ops {
        grouped.entry(target(op)).or_default().push(op);
    }

    let mut out = format!(
        "drift detected in {} table(s)/view(s) ({} change(s) total):\n",
        grouped.len(),
        ops.len()
    );
    for (name, ops) in grouped {
        out.push_str(&format!("\n{name}:\n"));
        for op in ops {
            out.push_str(&format!(
                "  [{}] {}\n",
                severity_label(op.destructiveness()),
                describe(op)
            ));
        }
    }
    out
}

fn render_unmapped(unmapped: &[UnmappedColumn]) -> String {
    let mut out = format!(
        "\n{} column(s) have a Postgres type cratestack could not confidently map to a \
         `.cstack` scalar — excluded from the comparison above, review manually:\n",
        unmapped.len()
    );
    for column in unmapped {
        out.push_str(&format!(
            "  {}.{}: {}\n",
            column.table, column.column, column.postgres_type
        ));
    }
    out
}

fn severity_label(destructiveness: Destructiveness) -> &'static str {
    match destructiveness {
        Destructiveness::Safe => "safe",
        Destructiveness::Lossy => "lossy",
        Destructiveness::Blocking => "blocking",
    }
}

/// The table or view name an op is "about", for grouping. Renames
/// group under their pre-rename (`prev`-side) name — the side that
/// exists in the live database being baselined.
fn target(op: &Op) -> &str {
    match op {
        Op::CreateTable(x) => &x.name,
        Op::DropTable(x) => &x.name,
        Op::AddColumn(x) => &x.table,
        Op::DropColumn(x) => &x.table,
        Op::AddIndex(x) => &x.table,
        Op::DropIndex(x) => &x.table,
        Op::AlterColumnType(x) => &x.table,
        Op::AlterColumnNullability(x) => &x.table,
        Op::AlterColumnDefault(x) => &x.table,
        Op::RenameTable(x) => &x.from,
        Op::RenameColumn(x) => &x.table,
        Op::AddCheck(x) => &x.table,
        Op::DropCheck(x) => &x.table,
        Op::AddForeignKey(x) => &x.table,
        Op::DropForeignKey(x) => &x.table,
        Op::CreateView(x) => &x.name,
        Op::DropView(x) => &x.name,
        Op::ReplaceView(x) => &x.name,
        Op::CreateMaterializedView(x) => &x.name,
        Op::DropMaterializedView(x) => &x.name,
    }
}

/// One line describing an op from a baseline drift report's point of
/// view: `diff_projections` was called `(live_database, schema)`, so
/// every "add"-shaped op means "in the schema but not the live
/// database" and every "drop"-shaped op means the reverse.
fn describe(op: &Op) -> String {
    match op {
        Op::CreateTable(x) => format!(
            "table `{}` is declared in the schema but does not exist in the live database",
            x.name
        ),
        Op::DropTable(x) => format!(
            "table `{}` exists in the live database but is not declared in the schema",
            x.name
        ),
        Op::AddColumn(x) => format!(
            "column `{}` is declared in the schema but does not exist in the live database",
            x.column.name
        ),
        Op::DropColumn(x) => format!(
            "column `{}` exists in the live database but is not declared in the schema",
            x.column
        ),
        Op::AddIndex(x) => format!(
            "index `{}` is declared in the schema but does not exist in the live database",
            x.name
        ),
        Op::DropIndex(x) => format!(
            "index `{}` exists in the live database but is not declared in the schema",
            x.name
        ),
        Op::AlterColumnType(x) => format!(
            "column `{}` type differs (live: {:?}, schema: {:?})",
            x.column, x.from, x.to
        ),
        Op::AlterColumnNullability(x) => format!(
            "column `{}` nullability differs (live: {:?}, schema: {:?})",
            x.column, x.from, x.to
        ),
        Op::AlterColumnDefault(x) => {
            format!(
                "column `{}` default value differs from the schema",
                x.column
            )
        }
        Op::RenameTable(x) => format!(
            "table `{}` in the live database matches `{}` in the schema by rename marker",
            x.from, x.to
        ),
        Op::RenameColumn(x) => format!(
            "column `{}` in the live database matches `{}` in the schema by rename marker",
            x.from, x.to
        ),
        Op::AddCheck(x) => format!(
            "CHECK `{}` is declared in the schema but does not exist in the live database",
            x.name
        ),
        Op::DropCheck(x) => format!(
            "CHECK `{}` exists in the live database but is not declared in the schema",
            x.name
        ),
        Op::AddForeignKey(x) => format!(
            "foreign key `{}` is declared in the schema but does not exist in the live database",
            x.name
        ),
        Op::DropForeignKey(x) => format!(
            "foreign key `{}` exists in the live database but is not declared in the schema",
            x.name
        ),
        Op::CreateView(x) => format!(
            "view `{}` is declared in the schema but does not exist in the live database",
            x.name
        ),
        Op::DropView(x) => format!(
            "view `{}` exists in the live database but is not declared in the schema",
            x.name
        ),
        Op::ReplaceView(x) => format!("view `{}` body differs from the schema", x.name),
        Op::CreateMaterializedView(x) => format!(
            "materialized view `{}` is declared in the schema but does not exist in the live \
             database",
            x.name
        ),
        Op::DropMaterializedView(x) => format!(
            "materialized view `{}` exists in the live database but is not declared in the \
             schema",
            x.name
        ),
    }
}

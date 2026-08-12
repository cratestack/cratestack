//! cratestack#513 verification crate — see `README.md` and this crate's
//! `Cargo.toml` doc comment for why this exists and why it is deliberately
//! **not** a workspace member.
//!
//! This module is the acceptance-bar proof itself: [`create_widget_with_note`]
//! composes two writes across two different models inside one
//! `db.transaction(...)` call using only the generated `Cratestack` handle.
//! Nothing in this file (or anywhere else in this crate) imports `sqlx`,
//! names `sqlx::Transaction`, or references any `sqlx::` path at all — the
//! closure's `tx` parameter type is inferred, not spelled out.

use cratestack::{CoolContext, CoolError};

cratestack::include_server_schema!("schema.cstack", db = Postgres);

pub use cratestack_schema as schema;

/// Creates a `Widget` and an attached `WidgetNote` atomically: both commit
/// together, or neither is visible afterwards. This is the exact shape
/// epic #488 / cratestack#513 exists to make reachable without a direct
/// `sqlx` dependency in a consuming service.
pub async fn create_widget_with_note(
    db: &schema::Cratestack,
    ctx: &CoolContext,
    widget_id: i64,
    label: String,
    note_id: i64,
    note: String,
) -> Result<(), CoolError> {
    db.transaction(async move |tx| {
        db.widget()
            .create(schema::CreateWidgetInput {
                id: widget_id,
                label,
            })
            .run_in_tx(tx, ctx)
            .await?;

        db.widget_note()
            .create(schema::CreateWidgetNoteInput {
                id: note_id,
                widgetId: widget_id,
                note,
            })
            .run_in_tx(tx, ctx)
            .await?;

        Ok(())
    })
    .await
}

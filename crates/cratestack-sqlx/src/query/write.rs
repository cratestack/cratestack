//! Write primitives — single-row `Create`/`Update`/`Delete`/`Upsert`
//! and predicate-driven bulk `UpdateMany`/`DeleteMany`. Every path
//! emits audit + event-outbox rows inside the same transaction as the
//! mutation, so a committed row always has its audit entry too.

mod create;
mod create_exec;
mod delete;
mod delete_exec;
mod delete_many;
mod delete_many_exec;
mod preview;
mod update;
mod update_exec;
mod update_many;
mod update_many_exec;
mod update_run;
mod upsert;
mod upsert_exec;
mod upsert_sql;

pub use create::CreateRecord;
pub use create_exec::create_record_with_executor;
pub use delete::DeleteRecord;
pub use delete_many::DeleteMany;
pub use preview::{render_update_many_preview_sql, render_update_preview_sql};
pub use update::{UpdateRecord, UpdateRecordSet};
pub use update_exec::update_record_with_executor;
pub use update_many::{UpdateMany, UpdateManySet};
pub use upsert::UpsertRecord;

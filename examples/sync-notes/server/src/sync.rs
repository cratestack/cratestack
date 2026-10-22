//! The two registries `cratestack-sync` needs, written against the
//! generated delegates so `schema.cstack`'s `@@allow` policies are the
//! guard on every pushed write and every pulled row. This file is what the
//! proposed `@@sync` attribute would generate (design doc §8).

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use cratestack::sqlx::PgTransaction;
use cratestack::{CratestackContext, CratestackError};
use cratestack_sync::protocol::RejectCode;
use cratestack_sync::server::{
    MutationContext, MutationError, MutationHandler, RowFetcher, SyncRegistry,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::cratestack_schema::{self as schema, Cratestack};

pub fn registry(db: Cratestack) -> SyncRegistry {
    SyncRegistry::new()
        .model("Note", Notes(db.clone()))
        .mutation("note.create", CreateNote(db.clone()))
        .mutation("note.update", UpdateNote(db.clone()))
        .mutation("note.delete", DeleteNote(db))
}

struct Notes(Cratestack);

#[async_trait]
impl RowFetcher for Notes {
    async fn fetch(
        &self,
        ctx: &CratestackContext,
        pks: &[String],
    ) -> Result<HashMap<String, Value>, CratestackError> {
        let ids: Vec<Uuid> = pks.iter().filter_map(|pk| pk.parse().ok()).collect();
        // `run(ctx)` applies the model's read policy: a row the puller may
        // not read is simply absent, and the puller receives a delete.
        let notes = self
            .0
            .note()
            .find_many()
            .where_(schema::note::id().in_(ids))
            .run(ctx)
            .await?;
        notes
            .into_iter()
            .map(|note| {
                let row = serde_json::to_value(&note)
                    .map_err(|e| CratestackError::Internal(e.to_string()))?;
                Ok((note.id.to_string(), row))
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct CreateArgs {
    id: Uuid,
    title: String,
}

struct CreateNote(Cratestack);

#[async_trait]
impl MutationHandler for CreateNote {
    async fn apply(
        &self,
        tx: &mut PgTransaction<'_>,
        mc: &MutationContext<'_>,
        args: Value,
    ) -> Result<(), MutationError> {
        let args: CreateArgs = serde_json::from_value(args)?;
        let title = checked_title(args.title)?;
        // The owner comes from the authenticated principal, never from the
        // args — and the create policy would refuse a mismatch anyway.
        self.0
            .note()
            .create(schema::CreateNoteInput {
                id: args.id,
                ownerId: mc.principal.to_owned(),
                title,
                done: false,
                updatedAt: Utc::now(),
            })
            .run_in_tx(tx, mc.ctx)
            .await?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct UpdateArgs {
    id: Uuid,
    title: Option<String>,
    done: Option<bool>,
}

struct UpdateNote(Cratestack);

#[async_trait]
impl MutationHandler for UpdateNote {
    async fn apply(
        &self,
        tx: &mut PgTransaction<'_>,
        mc: &MutationContext<'_>,
        args: Value,
    ) -> Result<(), MutationError> {
        let args: UpdateArgs = serde_json::from_value(args)?;
        let title = args.title.map(checked_title).transpose()?;
        self.0
            .note()
            .update(args.id)
            .set(schema::UpdateNoteInput {
                title,
                done: args.done,
                updatedAt: Some(Utc::now()),
                ..Default::default()
            })
            .run_in_tx(tx, mc.ctx)
            .await?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct DeleteArgs {
    id: Uuid,
}

struct DeleteNote(Cratestack);

#[async_trait]
impl MutationHandler for DeleteNote {
    async fn apply(
        &self,
        tx: &mut PgTransaction<'_>,
        mc: &MutationContext<'_>,
        args: Value,
    ) -> Result<(), MutationError> {
        let args: DeleteArgs = serde_json::from_value(args)?;
        self.0.note().delete(args.id).run_in_tx(tx, mc.ctx).await?;
        Ok(())
    }
}

fn checked_title(title: String) -> Result<String, MutationError> {
    let title = title.trim().to_owned();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(MutationError::Reject(
            RejectCode::Invalid,
            "title must be 1-200 characters".into(),
        ));
    }
    Ok(title)
}

//! Push: replay a client's mutations through the application's handlers.
//!
//! One Postgres transaction per push. Inside it, in order:
//!
//! 1. lock the client row and check the principal binding;
//! 2. drop seqs already consumed (a retried push after a lost response is
//!    acknowledged, never re-executed) and require the rest to continue
//!    contiguously from `last_mutation_seq + 1`;
//! 3. run each mutation in its own savepoint — a rejection rolls back that
//!    mutation's writes only and is reported, not raised;
//! 4. advance `last_mutation_seq` and write the client's ack into the
//!    change log, in the same transaction as the writes it acknowledges.
//!
//! Step 4 is why acks are a change rather than a field on this response:
//! a pull can only see the ack once it can also see the writes, so a client
//! never drops a pending mutation before the server state reflecting it.

use chrono::Utc;
use cratestack_core::CratestackContext;
use cratestack_sqlx::sqlx::{self, Acquire};
use serde_json::json;

use super::clients::authorize;
use super::registry::{MutationContext, MutationError};
use super::{SyncError, SyncServer, internal};
use crate::protocol::{
    ACK_MODEL, Mutation, MutationOutcome, MutationResult, PushRequest, PushResponse, RejectCode,
    SyncErrorCode, client_scope,
};

impl SyncServer {
    pub async fn push(
        &self,
        ctx: &CratestackContext,
        request: PushRequest,
    ) -> Result<PushResponse, SyncError> {
        let principal = self.principal(ctx)?;
        self.admit("push", &principal.id).await?;
        self.check_limits(&request.mutations)?;

        let mut tx = self.pool.begin().await.map_err(internal)?;
        let last_seq = authorize(&mut tx, &principal, request.client_id, true).await?;
        check_contiguous(&request.mutations, last_seq)?;

        let now = Utc::now();
        let mut next_seq = last_seq;
        let mut results = Vec::with_capacity(request.mutations.len());
        for mutation in request.mutations {
            if mutation.seq <= last_seq {
                results.push(MutationResult {
                    seq: mutation.seq,
                    outcome: MutationOutcome::AlreadyApplied,
                });
                continue;
            }
            let mc = MutationContext {
                ctx,
                principal: &principal.id,
                client_id: request.client_id,
                seq: mutation.seq,
            };
            let outcome = match self.screen(&mutation, now) {
                Some(rejection) => rejection,
                None => self.apply_one(&mut tx, &mc, mutation.clone()).await?,
            };
            next_seq = mutation.seq;
            results.push(MutationResult {
                seq: mutation.seq,
                outcome,
            });
        }

        if next_seq > last_seq {
            let seq = i64::try_from(next_seq).map_err(internal)?;
            sqlx::query(
                "UPDATE cratestack_sync_clients SET last_mutation_seq = $2 WHERE client_id = $1",
            )
            .bind(request.client_id)
            .bind(seq)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
            sqlx::query("SELECT cratestack_sync_record($1, $2, $3, 'upsert', $4)")
                .bind(client_scope(request.client_id))
                .bind(ACK_MODEL)
                .bind(request.client_id.to_string())
                .bind(json!({ "seq": next_seq }))
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;

        Ok(PushResponse {
            last_mutation_seq: next_seq,
            results,
        })
    }

    fn check_limits(&self, mutations: &[Mutation]) -> Result<(), SyncError> {
        if mutations.len() > self.policy.max_mutations_per_push {
            return Err(SyncError::new(
                SyncErrorCode::TooManyMutations,
                format!(
                    "{} mutations in one push (max {})",
                    mutations.len(),
                    self.policy.max_mutations_per_push
                ),
            ));
        }
        for mutation in mutations {
            let size = serde_json::to_vec(&mutation.args).map_or(usize::MAX, |b| b.len());
            if size > self.policy.max_mutation_bytes {
                return Err(SyncError::new(
                    SyncErrorCode::MutationTooLarge,
                    format!("mutation {} args are {size} bytes", mutation.seq),
                ));
            }
        }
        Ok(())
    }

    /// Rejections decided before any handler runs.
    fn screen(&self, mutation: &Mutation, now: chrono::DateTime<Utc>) -> Option<MutationOutcome> {
        let reject = |code, message: String| MutationOutcome::Rejected { code, message };
        if mutation.created_at > now + self.policy.max_clock_skew() {
            // Otherwise a client could post-date a mutation past the TTL.
            return Some(reject(
                RejectCode::Invalid,
                "createdAt is in the future".into(),
            ));
        }
        if now - mutation.created_at > self.policy.mutation_ttl() {
            return Some(reject(
                RejectCode::Expired,
                format!(
                    "mutation is older than the {}s mutation TTL",
                    self.policy.mutation_ttl_secs
                ),
            ));
        }
        if !self.registry.mutators.contains_key(&mutation.name) {
            return Some(reject(
                RejectCode::UnknownMutation,
                format!("no mutation named `{}`", mutation.name),
            ));
        }
        None
    }

    async fn apply_one(
        &self,
        tx: &mut sqlx::PgTransaction<'_>,
        mc: &MutationContext<'_>,
        mutation: Mutation,
    ) -> Result<MutationOutcome, SyncError> {
        let handler = self.registry.mutators[&mutation.name].clone();
        let mut savepoint = tx.begin().await.map_err(internal)?;
        match handler.apply(&mut savepoint, mc, mutation.args).await {
            Ok(()) => {
                savepoint.commit().await.map_err(internal)?;
                Ok(MutationOutcome::Applied)
            }
            Err(MutationError::Reject(code, message)) => {
                savepoint.rollback().await.map_err(internal)?;
                Ok(MutationOutcome::Rejected { code, message })
            }
            Err(MutationError::Fatal(error)) => Err(internal(error)),
        }
    }
}

fn check_contiguous(mutations: &[Mutation], last_seq: u64) -> Result<(), SyncError> {
    let fresh = mutations.iter().filter(|m| m.seq > last_seq);
    for (expected, mutation) in (last_seq + 1..).zip(fresh) {
        if mutation.seq != expected {
            let mut error = SyncError::new(
                SyncErrorCode::SequenceGap,
                format!("expected seq {expected}, got {}", mutation.seq),
            );
            error.last_mutation_seq = Some(last_seq);
            return Err(error);
        }
    }
    Ok(())
}

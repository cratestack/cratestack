//! The procedure registry of `fixtures/bigint_end_to_end.cstack`, shared by the
//! two REST test binaries that include that schema (`bigint_end_to_end.rs` and
//! `bigint_end_to_end_models.rs`). A registry has to implement every procedure
//! the schema declares, so it cannot be left empty in the second binary.
//!
//! Refers to `crate::cratestack_schema`, which `include_server_schema!` defines
//! at the root of each including test binary.

use cratestack::{BigInt, CratestackContext, CratestackError};

use crate::cratestack_schema::procedures::{bi_deposit, bi_echo, bi_find_accounts, bi_next};
use crate::cratestack_schema::{
    BiEcho, Cratestack, IsolatedCratestack, UpdateBiAccountInput, bi_account,
    build_bi_account_query_from_find_many,
};

#[derive(Clone)]
pub struct Procedures;

impl crate::cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn bi_echo(
        &self,
        _db: &Cratestack,
        _ctx: &CratestackContext,
        args: bi_echo::Args,
        _authorized: bi_echo::Authorized,
    ) -> Result<bi_echo::Output, CratestackError> {
        Ok(BiEcho {
            amountE8: args.args.amountE8,
            feeE8: args.args.feeE8,
            tagsE8: args.args.tagsE8,
        })
    }

    async fn bi_next(
        &self,
        _db: &Cratestack,
        _ctx: &CratestackContext,
        args: bi_next::Args,
        _authorized: bi_next::Authorized,
    ) -> Result<bi_next::Output, CratestackError> {
        args.valueE8
            .checked_add(BigInt::new(1))
            .ok_or_else(|| CratestackError::Validation("valueE8 overflows".to_owned()))
    }

    async fn bi_find_accounts(
        &self,
        db: &Cratestack,
        ctx: &CratestackContext,
        args: bi_find_accounts::Args,
        _authorized: bi_find_accounts::Authorized,
    ) -> Result<bi_find_accounts::Output, CratestackError> {
        build_bi_account_query_from_find_many(db, &args.query)
            .order_by(bi_account::id().asc())
            .run(ctx)
            .await
    }

    async fn bi_deposit(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: bi_deposit::Args,
        _authorized: bi_deposit::Authorized,
    ) -> Result<bi_deposit::Output, CratestackError> {
        let account = db
            .bi_account()
            .find_unique(args.args.accountId)
            .run(ctx)
            .await?
            .ok_or_else(|| CratestackError::NotFound("no such account".to_owned()))?;
        let amount = account
            .amountE8
            .checked_add(args.args.deltaE8)
            .ok_or_else(|| CratestackError::Validation("amountE8 overflows".to_owned()))?;
        db.bi_account()
            .update(args.args.accountId)
            .set(UpdateBiAccountInput {
                amountE8: Some(amount),
                ..Default::default()
            })
            .run(ctx)
            .await
    }
}

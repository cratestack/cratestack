//! The op-contract digest a sealed call binds (binding version 2;
//! cratestack#1123, EXT-14).
//!
//! The generated `Client::new` hands over the schema's `OP_CONTRACTS`; a
//! sealed call looks its own row up by the route it was made for and binds
//! that digest, so a schema edit that leaves this op's wire shape alone
//! cannot invalidate the call. The client also names the digest's first 8
//! bytes in the unbound `Cratestack-Contract` header, which lets a server
//! that accepts several digests for the op verify under the right one at
//! once (see `cratestack_core::ContractSelector`).

use cratestack_core::{ContractSelector, OpContracts, find_contract};

use crate::client::core::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::ClientError;

/// Where a sealed call finds its digest.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Contracts {
    /// The generated table, one digest per op (and `batch`).
    Table(OpContracts),
    /// One digest for every call: a hand-built client of one op.
    Pinned([u8; 32]),
}

impl Contracts {
    /// The digest of the call `method route`; `None` when the table has no
    /// such op.
    pub(crate) fn find(&self, method: &str, route: &str) -> Option<[u8; 32]> {
        match self {
            Contracts::Table(table) => find_contract(table, method, route).copied(),
            Contracts::Pinned(digest) => Some(*digest),
        }
    }
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// The digest a sealed call to `method route` binds, and the header
    /// value that selects it. Fails closed with `BadInput`: a call whose op
    /// the schema does not have is never sealed under another op's shape.
    pub(crate) fn contract_for(
        &self,
        method: &str,
        route: &str,
    ) -> Result<([u8; 32], String), ClientError> {
        let contracts = self.sealing.contracts.ok_or_else(|| {
            ClientError::BadInput(
                "a sealed call needs the op contract digests: use the generated client, or \
                 `with_contracts` / `with_contract_sha`"
                    .to_owned(),
            )
        })?;
        let digest = contracts.find(method, route).ok_or_else(|| {
            ClientError::BadInput(format!(
                "no contract digest for `{method} {route}`: the generated client's schema has no \
                 such op"
            ))
        })?;
        Ok((digest, ContractSelector::of(&digest).to_header_value()))
    }
}

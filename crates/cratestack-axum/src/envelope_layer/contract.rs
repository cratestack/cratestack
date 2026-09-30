//! Which op-contract digest a request is opened under (binding version 2;
//! cratestack#1123, EXT-14).
//!
//! The AAD binds the digest of the op the request calls, and is never sent.
//! The server holds, per op, every digest it accepts (current first, then
//! older compatible ones, newest first: [`AcceptedContracts`]), so it must
//! decide which one to rebuild the AAD with.
//!
//! 1. The client names one in the unbound `Cratestack-Contract` header
//!    ([`ContractSelector`]). It selects among digests already accepted; it
//!    cannot add one, and a lie is an ordinary `401` because the AAD
//!    carries the full 32 bytes.
//! 2. A selector that names no accepted digest is an **unsigned** `426
//!    contract_unsupported`, answered before any key is looked up. It
//!    reveals only that this op's shape is no longer served, which every
//!    client binary already carries, and it is unsigned, so a client treats
//!    it as a hint and never as proof.
//! 3. Without the header (a hand-built client) the accepted digests are
//!    tried newest first, at most [`EnvelopeLayerBuilder::max_contract_trials`]
//!    of them. A message's nonce is recorded only once it verifies, so a
//!    failed trial burns nothing; a forged message costs at most that many
//!    verifications.
//!
//! The response is sealed under the digest the request opened under, never
//! the server's current one.
//!
//! [`EnvelopeLayerBuilder::max_contract_trials`]: super::EnvelopeLayerBuilder::max_contract_trials

use bytes::Bytes;
use cratestack_core::{
    CONTRACT_HEADER, ContractSelector, CratestackError, UNAUTHENTICATED, find_contract,
};
use http::{HeaderMap, Method};

use super::layer::Config;
use super::opened::OpenedRequest;
use super::resolver::ResolvedRoute;
use super::seal::BindingInputs;

/// The accepted digests of `route`'s op: `None` when the table has no row
/// for it (a resolver that names a route the schema does not generate).
pub(super) fn accepted<'c>(
    config: &'c Config,
    method: &Method,
    route: &ResolvedRoute,
) -> Option<&'c [[u8; 32]]> {
    let key = route.contract_key().unwrap_or_else(|| route.op());
    find_contract(config.contracts, method.as_str(), key)
        .copied()
        .filter(|digests| !digests.is_empty())
}

/// The `Cratestack-Contract` header: absent, one selector, or a refusal
/// (malformed, or sent twice).
pub(super) fn selector(headers: &HeaderMap) -> Result<Option<ContractSelector>, CratestackError> {
    let mut values = headers.get_all(CONTRACT_HEADER).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return match headers.contains_key(CONTRACT_HEADER) {
            true => Err(CratestackError::BadRequest(format!(
                "{CONTRACT_HEADER} must be sent once"
            ))),
            false => Ok(None),
        };
    };
    ContractSelector::from_header_value(value.as_bytes()).map(Some)
}

/// What the request's digests are.
pub(super) enum Choice {
    /// Open under each, in order; never empty, never longer than the cap.
    Try(Vec<[u8; 32]>),
    /// The selector names no accepted digest: the unsigned `426`.
    Unsupported,
    /// The header is malformed: a `400`.
    Malformed(CratestackError),
    /// No digest is known for the route: the layer is misconfigured.
    Misconfigured,
}

pub(super) fn choose(
    config: &Config,
    method: &Method,
    route: &ResolvedRoute,
    headers: &HeaderMap,
) -> Choice {
    let Some(accepted) = accepted(config, method, route) else {
        return Choice::Misconfigured;
    };
    let cap = config.max_contract_trials;
    let candidates: Vec<[u8; 32]> = match selector(headers) {
        Err(error) => return Choice::Malformed(error),
        Ok(Some(selector)) => accepted
            .iter()
            .filter(|digest| selector.matches(digest))
            .take(cap)
            .copied()
            .collect(),
        Ok(None) => accepted.iter().take(cap).copied().collect(),
    };
    if candidates.is_empty() {
        Choice::Unsupported
    } else {
        Choice::Try(candidates)
    }
}

/// The digest an unsigned request's sealed response binds: the one its
/// selector names if that is accepted, else the op's current digest (the
/// unsigned request proves nothing, so a bad selector is not an error).
pub(super) fn for_unsigned(
    config: &Config,
    method: &Method,
    route: &ResolvedRoute,
    headers: &HeaderMap,
) -> Option<[u8; 32]> {
    let accepted = accepted(config, method, route)?;
    let named = selector(headers).ok().flatten().and_then(|selector| {
        accepted
            .iter()
            .find(|digest| selector.matches(digest))
            .copied()
    });
    named.or_else(|| accepted.first().copied())
}

/// Open `raw` under each candidate in turn, leaving `inputs` holding the
/// digest that verified, so the response seals under it. Only the coarse
/// `401` moves on to the next candidate; any other failure (a key backend
/// down) ends the search.
pub(super) async fn open_under(
    config: &Config,
    raw: &Bytes,
    inputs: &mut BindingInputs,
    candidates: &[[u8; 32]],
) -> Result<OpenedRequest, CratestackError> {
    let mut refused = CratestackError::Unauthorized(UNAUTHENTICATED.to_owned());
    for digest in candidates {
        inputs.contract = *digest;
        let params = inputs.params();
        let bind = inputs.binding(&params, None);
        match config.envelope.open_request(raw.clone(), &bind).await {
            Err(error @ CratestackError::Unauthorized(_)) => refused = error,
            other => return other,
        }
    }
    Err(refused)
}

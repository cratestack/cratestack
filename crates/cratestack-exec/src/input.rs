//! What an admission decision is made *about*: the op's declared
//! participation facts plus the per-call values a caller supplies.

use cratestack_core::{CratestackContext, OpDescriptor, RouteTransportDescriptor};

/// The participation facts a schema declares about one op, lifted off
/// whichever descriptor the schema's transport emitted.
///
/// REST schemas emit [`RouteTransportDescriptor`] and RPC schemas emit
/// [`OpDescriptor`]; never both. Both `From` impls exist so admission
/// logic is written once against this type rather than twice against two
/// descriptor shapes — the transport-parity rule in `CLAUDE.md`, and the
/// concrete lesson of cratestack#474, where a fix landed on one transport
/// and silently no-oped on the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpAdmission {
    /// Identifier for diagnostics only — nothing here dispatches on it.
    ///
    /// RPC descriptors carry a real dotted op id (`procedure.transfer`).
    /// REST descriptors carry no such thing, so the `From` impl below uses
    /// `RouteTransportDescriptor::name`, which is *not* unique across a
    /// model's five verbs. That is acceptable precisely because no
    /// decision reads this field; if one ever does, REST needs a genuine
    /// op id first.
    pub op_id: &'static str,
    /// `true` when the op does **not** participate in idempotency
    /// reservation — see the field of the same name on [`OpDescriptor`]
    /// for the full statement of what the flag means.
    pub idempotent_by_default: bool,
    /// `true` when the op participates in rate limiting. Carried but
    /// unread in slice 1 (rate limiting is still an L4 `tower::Layer`);
    /// it is here so a later slice does not have to change this type.
    pub rate_limited_by_default: bool,
}

impl OpAdmission {
    /// The facts to assume when a caller could not identify the op at all
    /// — no descriptor matched the request, or the caller has no
    /// descriptor table wired up.
    ///
    /// **Fails closed toward reserving**, which is the opposite direction
    /// from `cratestack-axum`'s rate-limit filters, and deliberately so:
    /// for rate limiting the conservative answer is "throttle it", while
    /// for idempotency the conservative answer is "take the reservation",
    /// because skipping a reservation is what would let a duplicate
    /// execute twice. Both mean "when in doubt, apply the protection".
    pub const fn unresolved() -> Self {
        Self {
            op_id: "",
            idempotent_by_default: false,
            rate_limited_by_default: true,
        }
    }
}

impl From<&'static OpDescriptor> for OpAdmission {
    fn from(descriptor: &'static OpDescriptor) -> Self {
        Self {
            op_id: descriptor.op_id,
            idempotent_by_default: descriptor.idempotent_by_default,
            rate_limited_by_default: descriptor.rate_limited_by_default,
        }
    }
}

impl From<&'static RouteTransportDescriptor> for OpAdmission {
    fn from(descriptor: &'static RouteTransportDescriptor) -> Self {
        Self {
            op_id: descriptor.name,
            idempotent_by_default: descriptor.idempotent_by_default,
            rate_limited_by_default: descriptor.rate_limited_by_default,
        }
    }
}

/// One call, as L3 sees it.
///
/// Borrowed throughout: every field is already owned by the caller for the
/// duration of the request, and copying a principal string plus a 32-byte
/// digest per call to satisfy a type would be a cost with no reader.
pub struct OpInput<'a> {
    pub op: OpAdmission,
    /// Namespace the idempotency key is scoped to. How it is derived is a
    /// caller concern — the HTTP adapter hashes `Authorization` or falls
    /// back to the verified TCP peer (cratestack#416), a future in-process
    /// caller would use something else entirely.
    pub principal: &'a str,
    /// `None` means the caller supplied no key, which admits without
    /// reserving. It is not an error: idempotency is opt-in per call.
    pub idempotency_key: Option<&'a str>,
    /// **Already computed by the caller, on purpose.** The digest covers
    /// method, path + query, content-type and body — all transport facts,
    /// none of which L3 may name (see the crate doc's first exclusion).
    /// Leaving the hash at the transport is what lets the HTTP adapter
    /// keep calling `cratestack_axum::idempotency::hash_request` with the
    /// same inputs in the same order, so the persisted digest — and
    /// therefore every replay/conflict decision keyed on it — is
    /// bit-for-bit what it was before this crate existed.
    pub fingerprint: [u8; 32],
    /// Always `None` in slice 1. Slice 3 fills it, when policy evaluation
    /// moves here and needs the authenticated principal's claims rather
    /// than just a namespace string.
    pub ctx: Option<&'a CratestackContext>,
}

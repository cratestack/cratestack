//! The roles of `@device_auth(role: "...")` (cratestack#1303): one per
//! method of fespalier's `DeviceAuthServer`, plus the optional
//! `sendEnrolmentCode`.

/// What one `@device_auth` procedure is in a device's ceremonies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeviceAuthRole {
    /// Optional: asks the server to send the one-time code, before leg one
    /// (for a server that checks the code before it issues a challenge).
    SendEnrolmentCode,
    /// `DeviceAuthServer.beginEnrolment`: the keys in, one challenge per key out.
    BeginEnrolment,
    /// `DeviceAuthServer.confirmEnrolment`: the signed challenges in, the
    /// device, its attestations and its user out.
    ConfirmEnrolment,
    /// `DeviceAuthServer.beginRenewal`: a device and a role in, a challenge out.
    BeginRenewal,
    /// `DeviceAuthServer.confirmRenewal`: the signature in, a new attestation out.
    ConfirmRenewal,
    /// `DeviceAuthServer.revoke`: the signed sign-out call.
    Revoke,
}

impl DeviceAuthRole {
    /// Every role, in the order error messages list them.
    pub const ALL: [Self; 6] = [
        Self::SendEnrolmentCode,
        Self::BeginEnrolment,
        Self::ConfirmEnrolment,
        Self::BeginRenewal,
        Self::ConfirmRenewal,
        Self::Revoke,
    ];

    /// The five roles `DeviceAuthServer` cannot be implemented without.
    pub const REQUIRED: [Self; 5] = [
        Self::BeginEnrolment,
        Self::ConfirmEnrolment,
        Self::BeginRenewal,
        Self::ConfirmRenewal,
        Self::Revoke,
    ];

    /// The role's name in the schema: the interface's method name.
    pub fn name(self) -> &'static str {
        match self {
            Self::SendEnrolmentCode => "sendEnrolmentCode",
            Self::BeginEnrolment => "beginEnrolment",
            Self::ConfirmEnrolment => "confirmEnrolment",
            Self::BeginRenewal => "beginRenewal",
            Self::ConfirmRenewal => "confirmRenewal",
            Self::Revoke => "revoke",
        }
    }

    /// The role written as `name`, if there is one.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.name() == name)
    }

    /// Whether the interface needs this role (all but `sendEnrolmentCode`).
    pub fn is_required(self) -> bool {
        self != Self::SendEnrolmentCode
    }

    /// Whether the call goes out with no session: every leg but `revoke`.
    /// `revoke` is signed by the device, so it must not be public.
    pub fn is_public(self) -> bool {
        self != Self::Revoke
    }
}

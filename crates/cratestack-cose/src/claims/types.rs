//! The values a claim object carries in and out.

use std::fmt;

use crate::alg::CoseAlg;
use crate::thumbprint::KID_LEN;

/// The longest `aud` (bytes of UTF-8) a claim object carries.
pub const MAX_AUD_LEN: usize = 256;
/// The longest `typ` (bytes of UTF-8) a claim object carries.
pub const MAX_TYP_LEN: usize = 128;
/// The longest `cti` (bytes) a claim object carries.
pub const MAX_CTI_LEN: usize = 64;

/// The CWT claims (RFC 8392 §3.1) a claim object may carry in protected
/// label `15` (RFC 9597). Only the claims that are `Some` are written.
///
/// All four are optional and none is interpreted by this crate: neither
/// [`sign_claims`](crate::sign_claims) nor [`open_claims`](crate::open_claims)
/// compares `exp` or `iat` with a clock, or remembers a `cti`. `exp` and
/// `iat` are whole seconds since the Unix epoch (`NumericDate`; fractional
/// and negative dates are not written and are refused when read).
///
/// ```
/// use cratestack_cose::CwtClaims;
///
/// let claims = CwtClaims { exp: Some(1_790_003_600), ..CwtClaims::default() };
/// assert_eq!(claims.aud, None);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CwtClaims {
    /// `aud` (3): who the object is for, a single non-empty string of at most
    /// [`MAX_AUD_LEN`] bytes. The array form of RFC 8392 is not accepted.
    pub aud: Option<String>,
    /// `exp` (4): the last second the object is meant to be honoured. The
    /// consumer enforces it.
    pub exp: Option<u64>,
    /// `iat` (6): when it was issued, by the issuer's clock.
    pub iat: Option<u64>,
    /// `cti` (7): a unique id of 1 to [`MAX_CTI_LEN`] bytes, for a consumer
    /// that records ids it has accepted.
    pub cti: Option<Vec<u8>>,
}

/// A claim object whose signature verified.
///
/// **Verified is not fresh.** Nothing here says the object is unexpired,
/// issued for this audience or seen for the first time: that is the
/// caller's to decide from [`claims`](Self::claims). `Debug` prints the
/// payload's length, not its bytes.
///
/// `#[non_exhaustive]`: read it, never build it.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenedClaims {
    /// The `kid` from the protected header; the verifying key's own `kid`.
    pub kid: [u8; KID_LEN],
    /// The algorithm the object was verified with (a signature algorithm).
    pub alg: CoseAlg,
    /// The RFC 9679 thumbprint of the key that verified. Two keys can share
    /// a `kid`, so this, not the `kid`, identifies the signer.
    pub key_thumbprint: [u8; 32],
    /// The claims from the protected header.
    pub claims: CwtClaims,
    /// The payload, exactly as signed.
    pub payload: Vec<u8>,
}

impl fmt::Debug for OpenedClaims {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenedClaims")
            .field("kid", &self.kid)
            .field("alg", &self.alg)
            .field("key_thumbprint", &self.key_thumbprint)
            .field("claims", &self.claims)
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

//! The closed set of algorithms and message structures (ADR 0006 §3, as
//! amended while scoping P0).

/// A COSE algorithm this crate signs or verifies with.
///
/// **Deviation from the ADR 0006 §1 sketch**, which types `alg` as
/// `coset::iana::Algorithm`: that is an open registry of about a hundred
/// values, so every verifier would need its own allowlist and every
/// resolver would be asked about algorithms it can never honour. Here the
/// allowlist *is* the type. [`CoseAlg::from_id`] is the only way a wire
/// value becomes an algorithm, and it knows four ids. The deprecated
/// polymorphic ids -8 (EdDSA) and -7 (ES256), which RFC 9864 replaces with
/// the fully specified -19 and -9, are rejected like any other unknown id:
/// nothing has shipped with them, so there is nothing to stay compatible
/// with.
///
/// `#[non_exhaustive]` because Q5 reserves a hybrid post-quantum `alg`
/// value: adding it is a new variant, not a wire break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoseAlg {
    /// `-19`, Ed25519 (RFC 9864). The default for COSE_Sign1 (Q2).
    Ed25519,
    /// `-9`, ESP256: ECDSA over P-256 with SHA-256 (RFC 9864). Opt-in, for
    /// keys held in a KMS, an HSM or as non-extractable WebCrypto keys (Q2).
    Esp256,
    /// `4`, HMAC 256/64: HMAC-SHA-256 truncated to its first 8 bytes. Safe
    /// only because a forgery needs online attempts, which are rate
    /// limited (§3).
    Hmac256_64,
    /// `5`, HMAC 256/256.
    Hmac256_256,
}

impl CoseAlg {
    /// Every accepted algorithm, for tests and for exhaustive tables.
    pub const ALL: [CoseAlg; 4] = [
        CoseAlg::Ed25519,
        CoseAlg::Esp256,
        CoseAlg::Hmac256_64,
        CoseAlg::Hmac256_256,
    ];

    /// The IANA "COSE Algorithms" value written into protected label 1.
    pub const fn id(self) -> i64 {
        match self {
            CoseAlg::Ed25519 => -19,
            CoseAlg::Esp256 => -9,
            CoseAlg::Hmac256_64 => 4,
            CoseAlg::Hmac256_256 => 5,
        }
    }

    /// The algorithm for a wire value, or `None` for anything outside the
    /// allowlist, including -8 and -7.
    pub const fn from_id(id: i64) -> Option<CoseAlg> {
        match id {
            -19 => Some(CoseAlg::Ed25519),
            -9 => Some(CoseAlg::Esp256),
            4 => Some(CoseAlg::Hmac256_64),
            5 => Some(CoseAlg::Hmac256_256),
            _ => None,
        }
    }

    /// The message structure this algorithm belongs to. A tag-17 message
    /// carrying a signature algorithm, or a tag-18 message carrying a MAC
    /// algorithm, is rejected before any key is looked up.
    pub const fn mode(self) -> CoseMode {
        match self {
            CoseAlg::Ed25519 | CoseAlg::Esp256 => CoseMode::Sign1,
            CoseAlg::Hmac256_64 | CoseAlg::Hmac256_256 => CoseMode::Mac0,
        }
    }

    /// The exact length of the signature or tag, in bytes. Q5: nothing in
    /// this crate assumes 64; every size is read from here. Both signature
    /// algorithms happen to be 64 (Ed25519, and ESP256's fixed-size `r‖s`).
    pub const fn signature_len(self) -> usize {
        match self {
            CoseAlg::Ed25519 | CoseAlg::Esp256 => 64,
            CoseAlg::Hmac256_64 => 8,
            CoseAlg::Hmac256_256 => 32,
        }
    }
}

/// The unary message structure: COSE_Sign1 or COSE_Mac0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoseMode {
    /// COSE_Sign1, CBOR tag 18. Asymmetric: the verifier cannot forge.
    Sign1,
    /// COSE_Mac0, CBOR tag 17. Symmetric: service to service inside one
    /// trust domain only (§3, "Why not Mac0 for devices?").
    Mac0,
}

impl CoseMode {
    /// The CBOR tag every message of this mode carries. Messages are
    /// always tagged (§3), so the structure is never inferred from context.
    pub const fn tag(self) -> u64 {
        match self {
            CoseMode::Sign1 => 18,
            CoseMode::Mac0 => 17,
        }
    }

    /// The `Content-Type` of a unary body in this mode (§2, RFC 9052 §2).
    pub const fn media_type(self) -> &'static str {
        match self {
            CoseMode::Sign1 => "application/cose; cose-type=\"cose-sign1\"",
            CoseMode::Mac0 => "application/cose; cose-type=\"cose-mac0\"",
        }
    }

    /// The context string that opens the `Sig_structure` / `MAC_structure`
    /// (RFC 9052 §4.4, §6.3).
    pub(crate) const fn context(self) -> &'static str {
        match self {
            CoseMode::Sign1 => "Signature1",
            CoseMode::Mac0 => "MAC0",
        }
    }
}

//! Local-development certificates (design §6a.5). Browsers accept a
//! self-signed certificate for WebTransport only through
//! `serverCertificateHashes`, and only when it is valid for at most 14
//! days. This generates one valid for 10 (a day of backdating included, to
//! absorb clock skew), plus the hash the page must pass.

use cratestack_core::CratestackError;
use sha2::{Digest, Sha256};

use crate::transport::{CertificateDer, PrivateKeyDer};

/// A short-lived self-signed certificate and its SHA-256 hash.
pub struct DevCertificate {
    pub certificate_chain: Vec<CertificateDer<'static>>,
    pub private_key: PrivateKeyDer<'static>,
    /// SHA-256 of the certificate's DER: the `value` of a browser's
    /// `serverCertificateHashes` entry (`algorithm: "sha-256"`).
    pub sha256: [u8; 32],
}

impl DevCertificate {
    /// For `hosts` (DNS names or IP literals), ECDSA P-256.
    pub fn generate(hosts: &[&str]) -> Result<Self, CratestackError> {
        let error = |e: rcgen::Error| CratestackError::Internal(format!("dev certificate: {e}"));
        let mut params = rcgen::CertificateParams::new(
            hosts.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>(),
        )
        .map_err(error)?;
        let now = time::OffsetDateTime::now_utc();
        params.not_before = now - time::Duration::days(1);
        params.not_after = now + time::Duration::days(9);
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(error)?;
        let certificate = params.self_signed(&key).map_err(error)?;
        let der = certificate.der().to_vec();
        Ok(Self {
            sha256: Sha256::digest(&der).into(),
            certificate_chain: vec![CertificateDer::from(der)],
            private_key: PrivateKeyDer::Pkcs8(key.serialize_der().into()),
        })
    }
}

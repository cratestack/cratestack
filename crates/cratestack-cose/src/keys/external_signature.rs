//! What an external signer's callback may hand back, normalised to the
//! bytes COSE carries (cratestack#1007).

use cratestack_core::CratestackError;
use p256::ecdsa::Signature;

/// An Ed25519 signature is exactly 64 bytes: anything else is the
/// callback's failure, not the message's.
pub(super) fn ed25519_signature(signature: Vec<u8>) -> Result<Vec<u8>, CratestackError> {
    if signature.len() == 64 {
        Ok(signature)
    } else {
        Err(CratestackError::Internal(
            "the external signer did not return a 64-byte Ed25519 signature".to_owned(),
        ))
    }
}

/// The 64-byte `r ‖ s` for a signature that is either that or DER.
///
/// DER is tried first when the bytes start a DER `SEQUENCE` (`0x30`) and
/// parse as one; a raw signature that happens to start with `0x30` does not
/// parse as a DER signature of the same length except with a probability that
/// is negligible for a real signature, so in practice the two are not
/// confused; anything that is neither is refused.
pub(super) fn raw_signature(signature: &[u8]) -> Result<Vec<u8>, CratestackError> {
    let parsed = match signature {
        [0x30, ..] => Signature::from_der(signature).ok(),
        _ => None,
    }
    .or_else(|| Signature::from_slice(signature).ok());
    parsed
        .map(|signature| signature.to_bytes().to_vec())
        .ok_or_else(|| {
            CratestackError::Internal(
                "the external signer returned neither DER nor a raw 64-byte P-256 signature"
                    .to_owned(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::raw_signature;

    fn padded(value: &[u8]) -> Vec<u8> {
        let mut out = vec![0; 32 - value.len()];
        out.extend_from_slice(value);
        out
    }

    #[test]
    fn a_high_bit_r_and_s_lose_their_zero_padding() {
        // INTEGERs whose top bit is set carry a leading 0x00 in DER: 33 bytes.
        let (mut r, mut s) = (vec![0x80], vec![0xC0]);
        r.extend([0x11; 31]);
        s.extend([0x22; 31]);
        let mut der = vec![0x30, 0x46, 0x02, 0x21, 0x00];
        der.extend(&r);
        der.extend([0x02, 0x21, 0x00]);
        der.extend(&s);
        assert_eq!(raw_signature(&der).expect("converted"), [r, s].concat());
    }

    #[test]
    fn a_short_r_and_s_are_left_padded_to_32_bytes() {
        // r = 0x0102 and s = 0x7f: DER drops the leading zeros.
        let der = [0x30, 0x07, 0x02, 0x02, 0x01, 0x02, 0x02, 0x01, 0x7f];
        assert_eq!(
            raw_signature(&der).expect("converted"),
            [padded(&[0x01, 0x02]), padded(&[0x7f])].concat()
        );
    }

    #[test]
    fn garbage_is_refused() {
        assert!(matches!(
            raw_signature(&[0x30, 0x01]),
            Err(cratestack_core::CratestackError::Internal(_))
        ));
    }
}

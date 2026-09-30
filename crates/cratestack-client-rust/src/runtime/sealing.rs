//! How a [`RuntimeHandle`](super::handle::RuntimeHandle) gets its envelope
//! (cratestack#1007), and the checks that keep the config and the envelope
//! from disagreeing.
//!
//! The keys cannot cross the FFI in `RuntimeConfigWire`, so the config only
//! *names* the envelope (`CoseSign1`, `CoseMac0`) and the host hands the
//! envelope itself to `RuntimeHandle::with_envelope`. That leaves two ways to
//! get it wrong, both refused before any request is built: naming an
//! envelope and supplying none, and supplying one the config does not name.

use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::runtime::wire::{RuntimeEnvelopeConfig, RuntimeErrorCode, RuntimeErrorWire};

pub(super) fn bad_input(message: String) -> RuntimeErrorWire {
    RuntimeErrorWire {
        code: RuntimeErrorCode::BadInput,
        http_status: None,
        message,
        remote_code: None,
        remote_body: None,
    }
}

pub(super) enum Sealing {
    None,
    #[cfg(feature = "cose")]
    Cose(
        crate::envelope::ClientEnvelope,
        cratestack_core::OpContracts,
    ),
}

impl Sealing {
    /// The config and the supplied envelope name the same thing.
    pub(super) fn agrees_with(
        &self,
        named: &RuntimeEnvelopeConfig,
    ) -> Result<(), RuntimeErrorWire> {
        match (self, named) {
            (Sealing::None, RuntimeEnvelopeConfig::None) => Ok(()),
            (Sealing::None, named) => Err(bad_input(format!(
                "the config names {named:?}, but its keys cannot travel in the config: build the \
                 handle with `RuntimeHandle::with_envelope` (feature `cose`)"
            ))),
            #[cfg(feature = "cose")]
            (Sealing::Cose(envelope, _), named) => {
                use cratestack_cose::CoseMode;
                let expected = match envelope.mode() {
                    CoseMode::Sign1 => RuntimeEnvelopeConfig::CoseSign1,
                    CoseMode::Mac0 => RuntimeEnvelopeConfig::CoseMac0,
                };
                if *named == expected {
                    Ok(())
                } else {
                    Err(bad_input(format!(
                        "the config names {named:?}, but the envelope supplied is {expected:?}"
                    )))
                }
            }
        }
    }

    /// Give `client` the envelope, if there is one.
    pub(super) fn apply<C: HttpClientCodec>(
        self,
        client: CratestackClient<C>,
    ) -> Result<CratestackClient<C>, RuntimeErrorWire> {
        match self {
            Sealing::None => Ok(client),
            #[cfg(feature = "cose")]
            Sealing::Cose(envelope, contracts) => client
                .with_envelope(envelope)
                .map(|client| client.with_contracts(contracts))
                .map_err(RuntimeErrorWire::from),
        }
    }
}

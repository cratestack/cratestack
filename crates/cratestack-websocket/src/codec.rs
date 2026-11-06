//! One frame per WebSocket message (design §6.2): binary messages carry
//! CBOR, text messages JSON, encoded by the same `CborCodec` / `JsonCodec`
//! as every other binding.

use axum::extract::ws::Message;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::subscription_wire::{ClientFrame, ServerFrame};
use cratestack_core::{CratestackCodec, CratestackError};

/// Subprotocol for CBOR frames.
pub const CBOR_PROTOCOL: &str = "cratestack.rpc.v1+cbor";
/// Subprotocol for JSON frames.
pub const JSON_PROTOCOL: &str = "cratestack.rpc.v1+json";

/// The codec one socket speaks, chosen at the upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireCodec {
    Cbor,
    Json,
}

impl WireCodec {
    pub fn protocol(self) -> &'static str {
        match self {
            Self::Cbor => CBOR_PROTOCOL,
            Self::Json => JSON_PROTOCOL,
        }
    }

    /// The first offered subprotocol this server speaks, else the `codec`
    /// query parameter, else CBOR. `negotiated` is `true` when the choice
    /// came from a subprotocol, which the upgrade response must then echo.
    pub fn select(
        offered: Option<&str>,
        query: Option<&str>,
    ) -> Result<(Self, bool), CratestackError> {
        for protocol in offered.into_iter().flat_map(|header| header.split(',')) {
            match protocol.trim() {
                CBOR_PROTOCOL => return Ok((Self::Cbor, true)),
                JSON_PROTOCOL => return Ok((Self::Json, true)),
                _ => {}
            }
        }
        let from_query = query
            .into_iter()
            .flat_map(|q| q.split('&'))
            .find_map(|pair| pair.strip_prefix("codec="));
        match from_query {
            None | Some("cbor") => Ok((Self::Cbor, false)),
            Some("json") => Ok((Self::Json, false)),
            Some(other) => Err(CratestackError::NotAcceptable(format!(
                "unsupported WebSocket codec `{other}`; expected `cbor` or `json`",
            ))),
        }
    }

    pub fn encode(self, frame: &ServerFrame) -> Result<Message, CratestackError> {
        Ok(match self {
            Self::Cbor => Message::Binary(CborCodec.encode(frame)?.into()),
            Self::Json => Message::Text(
                serde_json::to_string(frame)
                    .map_err(|error| CratestackError::Codec(error.to_string()))?
                    .into(),
            ),
        })
    }

    /// A data message as a client frame. A binary message on a JSON socket,
    /// or text on a CBOR one, is a protocol error.
    pub fn decode(self, message: &Message) -> Result<ClientFrame, CratestackError> {
        match (self, message) {
            (Self::Cbor, Message::Binary(bytes)) => CborCodec.decode(bytes),
            (Self::Json, Message::Text(text)) => serde_json::from_str(text.as_str())
                .map_err(|error| CratestackError::BadRequest(format!("invalid frame: {error}"))),
            _ => Err(CratestackError::BadRequest(format!(
                "this socket speaks {}: binary messages for CBOR, text for JSON",
                self.protocol(),
            ))),
        }
    }
}

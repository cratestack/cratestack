//! The byte work of a compact call: the `Accept` it sends, the request body
//! it may send compact, and turning a compact response back into the named
//! bytes the normal decode reads.

use std::sync::Arc;

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

use cratestack_codec_cbor::CborSeqChunkDecoder;
use cratestack_codec_cbor::compact::{cbor_from_compact, cbor_to_compact};
use cratestack_codec_json::compact::{json_from_compact, json_to_compact};
use cratestack_core::CratestackError;
use cratestack_core::compact::{
    COMPACT_CBOR, COMPACT_CBOR_SEQ, COMPACT_JSON, Direction, compact_media_type_for,
    plain_media_type_for,
};

use crate::client::compact::{CompactCall, CompactClient};
use crate::codec::media_type_matches;
use crate::error::ClientError;
use crate::runtime::wire::{RuntimeHeader, RuntimeResponseWire};

/// Tag of the in-band stream error item; never rewritten.
const STREAM_ERROR_TAG: u64 = 48900;

/// A request as it goes out: body, its `Content-Type` and its `Accept`.
pub(crate) struct Prepared {
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) content_type: &'static str,
    pub(crate) accept: String,
}

impl CompactCall {
    /// `Accept: <compact types>, <plain types>`. A stream (`seq_only`)
    /// asks only for the compact sequence: its pump reads items one by one.
    fn accept(&self, plain: &str, seq_only: bool) -> String {
        let compact = plain
            .split(',')
            .map(str::trim)
            .filter(|entry| !seq_only || entry.ends_with("cbor-seq"))
            .filter_map(compact_media_type_for);
        let mut out: Vec<&str> = compact.collect();
        out.push(plain);
        out.join(", ")
    }

    pub(crate) fn prepare(
        &self,
        body: Option<Vec<u8>>,
        content_type: &'static str,
        plain_accept: &str,
        seq_only: bool,
    ) -> Prepared {
        let accept = self.accept(plain_accept, seq_only);
        let (body, content_type) = match body {
            Some(named) if self.send_compact => match self.encode_request(content_type, &named) {
                Some((compact, ct)) => (Some(compact), ct),
                None => (Some(named), content_type),
            },
            other => (other, content_type),
        };
        Prepared {
            body,
            content_type,
            accept,
        }
    }

    /// The compact form of `named`, or `None` (a codec with no compact
    /// type, or a body the tree cannot rewrite) to send it named.
    fn encode_request(&self, content_type: &str, named: &[u8]) -> Option<(Vec<u8>, &'static str)> {
        let tables = &self.state.tables;
        let node = self.state.node(self.index, Direction::Request);
        let target = compact_media_type_for(content_type)?;
        let bytes = if target == COMPACT_CBOR {
            cbor_to_compact(tables, node, named)
        } else if target == COMPACT_JSON {
            json_to_compact(tables, node, named)
        } else {
            return None;
        };
        bytes.ok().map(|bytes| (bytes, target))
    }

    /// Turn a buffered response back into the plain one, and update the
    /// op's flag: a compact 2xx turns compact requests on, a plain 2xx or
    /// a `415` turns them off. Error bodies are never compact.
    pub(crate) fn open_response(
        &self,
        mut response: RuntimeResponseWire,
    ) -> Result<RuntimeResponseWire, ClientError> {
        if !(200..=299).contains(&response.status_code) {
            self.note_failure(response.status_code);
            return Ok(response);
        }
        let content_type = header(&response.headers, "content-type").unwrap_or_default();
        let Some(plain) = plain_media_type_for(content_type.split(';').next().unwrap_or("").trim())
        else {
            self.state.set_compact(&self.op, false);
            return Ok(response);
        };
        let compact_type = content_type.clone();
        self.state.set_compact(&self.op, true);
        let node = self.state.node(self.index, Direction::Response);
        let tables = &self.state.tables;
        response.body = if media_type_matches(&compact_type, COMPACT_CBOR_SEQ) {
            let mut decoder = CborSeqChunkDecoder::new();
            let mut out = Vec::new();
            for item in decoder.feed_chunk(&response.body)? {
                out.extend(self.item_transcoder().item(&item)?);
            }
            if decoder.pending_len() > 0 {
                return Err(ClientError::InvalidResponse(
                    "compact sequence ended inside an item".to_owned(),
                ));
            }
            out
        } else if media_type_matches(&compact_type, COMPACT_JSON) {
            json_from_compact(tables, node, &response.body)?
        } else {
            cbor_from_compact(tables, node, &response.body)?
        };
        for entry in &mut response.headers {
            if entry.name.eq_ignore_ascii_case("content-type") {
                entry.value = plain.to_owned();
            }
        }
        Ok(response)
    }

    /// A non-2xx answer: a `415` says the server cannot read compact
    /// bodies (any more), so this op goes back to named requests.
    pub(crate) fn note_failure(&self, status: u16) {
        if status == 415 {
            self.state.set_compact(&self.op, false);
        }
    }

    /// `Cratestack-Contract`: which contract's key tables this call speaks.
    pub(crate) fn stamp_selector(&self, headers: &mut HeaderMap) -> Result<(), ClientError> {
        let value = HeaderValue::from_str(&self.selector).map_err(|error| {
            ClientError::BadInput(format!("invalid contract selector: {error}"))
        })?;
        headers.insert(HeaderName::from_static("cratestack-contract"), value);
        Ok(())
    }

    /// A successful stream response: updates the op's flag from its
    /// `Content-Type` and returns the item rewrite when it is compact.
    pub(crate) fn note_stream(&self, headers: &HeaderMap) -> Option<ItemTranscoder> {
        let content_type = headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let compact = media_type_matches(content_type, COMPACT_CBOR_SEQ);
        self.state.set_compact(&self.op, compact);
        compact.then(|| self.item_transcoder())
    }

    /// The per-item rewrite of a compact sequence, for the stream pump.
    pub(crate) fn item_transcoder(&self) -> ItemTranscoder {
        ItemTranscoder {
            state: Arc::clone(&self.state),
            index: self.index,
        }
    }
}

fn header(headers: &[RuntimeHeader], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(name))
        .map(|entry| entry.value.clone())
}

/// Turns one compact sequence item back into a named one.
#[derive(Clone)]
pub(crate) struct ItemTranscoder {
    state: Arc<CompactClient>,
    index: usize,
}

impl ItemTranscoder {
    pub(crate) fn item(&self, item: &[u8]) -> Result<Vec<u8>, CratestackError> {
        if is_stream_error(item) {
            return Ok(item.to_vec());
        }
        let node = self.state.node(self.index, Direction::Response);
        cbor_from_compact(&self.state.tables, node, item)
    }
}

fn is_stream_error(item: &[u8]) -> bool {
    let mut decoder = minicbor::Decoder::new(item);
    matches!(decoder.datatype(), Ok(minicbor::data::Type::Tag))
        && decoder
            .tag()
            .is_ok_and(|tag| tag.as_u64() == STREAM_ERROR_TAG)
}

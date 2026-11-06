use super::compact::CompactCall;
use super::prepared::{PreparedCall, bad};
use crate::{
    ClientError, CratestackClient, HeaderPair, HttpClientCodec, QueryPair, RouteRef, RuntimeHeader,
    RuntimeResponseWire, TypedResponse,
};
use cratestack_core::compact::{COMPACT_CBOR, COMPACT_JSON};
use reqwest::Method;
use serde::{Serialize, de::DeserializeOwned};

impl<C: HttpClientCodec> CratestackClient<C> {
    /// Encode and freeze one keyed unary mutation before any network I/O.
    /// Requires compact key tables, even when this first payload stays named.
    /// Persist the descriptor before calling `execute_prepared`; the framework
    /// does not enqueue it. Credentials come from the authorizer on each attempt.
    pub fn prepare_call<Input: Serialize>(
        &self,
        method: &str,
        route: RouteRef<'_>,
        path: &str,
        input: &Input,
        query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<PreparedCall, ClientError> {
        if !matches!(C::CONTENT_TYPE, "application/cbor" | "application/json") {
            return Err(bad("prepared calls require a CBOR or JSON codec"));
        }
        let method = Method::from_bytes(method.as_bytes()).map_err(|e| bad(e.to_string()))?;
        let scoped = self.at(route);
        let compact = scoped
            .compact_call(&method, headers, true)
            .ok_or_else(|| bad("prepare_call requires compact tables for this operation"))?;
        let plain_accept = if self.is_sealed() {
            self.codec.payload_accept()
        } else {
            self.codec.accept_header_value()
        };
        let sent = compact.prepare(
            Some(self.codec.encode(input)?),
            C::CONTENT_TYPE,
            plain_accept,
            false,
        );
        let call = PreparedCall {
            version: 1,
            base_url: self.config.base_url.to_string(),
            method: method.to_string(),
            path: path.to_owned(),
            route: route.template().to_owned(),
            params: route.params().iter().map(|s| (*s).to_owned()).collect(),
            query: if query.is_empty() {
                None
            } else {
                Some(serde_urlencoded::to_string(query).map_err(|e| bad(e.to_string()))?)
            },
            headers: headers
                .iter()
                .map(|(name, value)| RuntimeHeader {
                    name: (*name).to_owned(),
                    value: (*value).to_owned(),
                })
                .collect(),
            body: sent.body.unwrap_or_default(),
            content_type: sent.content_type.to_owned(),
            plain_type: C::CONTENT_TYPE.to_owned(),
            accept: sent.accept,
            contract: compact.state.tables.ops[compact.index].digest,
            key_tables: compact.state.tables.to_bytes(),
            op: compact.op,
            sealed: self.is_sealed(),
        };
        call.validate()?;
        Ok(call)
    }

    /// Send original inner bytes, reauthorizing and (when configured) resealing
    /// with fresh COSE claims. No compact negotiation or request transcoding runs.
    /// A changed service, codec, seal mode, or op contract fails before sending.
    pub async fn execute_prepared(
        &self,
        call: &PreparedCall,
    ) -> Result<RuntimeResponseWire, ClientError> {
        call.validate()?;
        if call.base_url != self.config.base_url.as_str()
            || call.plain_type != C::CONTENT_TYPE
            || call.sealed != self.is_sealed()
        {
            return Err(bad(
                "prepared service, codec or sealing mode differs from this client",
            ));
        }
        let params = call.params.iter().map(String::as_str).collect::<Vec<_>>();
        let scoped = self.at(RouteRef::new(&call.route, &params));
        scoped.prepared_compact(call)?;
        let headers = call
            .headers
            .iter()
            .map(|h| (h.name.as_str(), h.value.as_str()))
            .collect::<Vec<_>>();
        scoped
            .send_buffered(
                Method::from_bytes(call.method.as_bytes()).map_err(|e| bad(e.to_string()))?,
                &call.path,
                Some(call.body.clone()),
                call.query.as_deref(),
                &headers,
                None,
                Some(call),
            )
            .await
    }

    /// Typed equivalent of `execute_prepared`, retaining replay/status headers.
    pub async fn execute_prepared_typed<Output: DeserializeOwned>(
        &self,
        call: &PreparedCall,
    ) -> Result<TypedResponse<Output>, ClientError> {
        let wire = self.execute_prepared(call).await?;
        super::decode::decode_typed_response_with_metadata(&self.codec, &wire)
    }

    pub(super) fn prepared_compact(&self, call: &PreparedCall) -> Result<CompactCall, ClientError> {
        let state = self
            .compact
            .as_ref()
            .ok_or_else(|| bad("prepared call needs its contract key tables"))?;
        let index = state
            .tables
            .ops
            .iter()
            .position(|keys| keys.op == call.op && keys.digest == call.contract)
            .ok_or_else(|| bad("prepared op contract differs; restore with its original schema"))?;
        call.wire_type::<C>()?;
        if call.key_tables != state.tables.to_bytes() {
            return Err(bad("prepared key tables differ from this client"));
        }
        let compact = CompactCall {
            state: state.clone(),
            op: call.op.clone(),
            index,
            send_compact: false,
            selector: cratestack_core::ContractSelector::of(&call.contract).to_header_value(),
        };
        let plain_accept = if self.is_sealed() {
            self.codec.payload_accept()
        } else {
            self.codec.accept_header_value()
        };
        if call.accept
            != compact
                .prepare(None, C::CONTENT_TYPE, plain_accept, false)
                .accept
        {
            return Err(bad("prepared accept differs from this client"));
        }
        Ok(compact)
    }

    pub(super) fn is_sealed(&self) -> bool {
        #[cfg(feature = "cose")]
        {
            self.sealing.envelope.is_some()
        }
        #[cfg(not(feature = "cose"))]
        {
            false
        }
    }
}

impl PreparedCall {
    pub(super) fn wire_type<C: HttpClientCodec>(&self) -> Result<&'static str, ClientError> {
        if self.content_type == C::CONTENT_TYPE {
            return Ok(C::CONTENT_TYPE);
        }
        match (C::CONTENT_TYPE, self.content_type.as_str()) {
            ("application/cbor", COMPACT_CBOR) => Ok(COMPACT_CBOR),
            ("application/json", COMPACT_JSON) => Ok(COMPACT_JSON),
            _ => Err(bad("prepared payload type is incompatible with the codec")),
        }
    }
}

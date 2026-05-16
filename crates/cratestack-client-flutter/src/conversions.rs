//! Newtype `From` conversions between Flutter-facing and core wire types.

use std::path::PathBuf;

use cratestack_client_rust::{
    ClientError, RuntimeChunkWire, RuntimeCodecConfig, RuntimeConfigWire, RuntimeEnvelopeConfig,
    RuntimeErrorCode, RuntimeErrorWire, RuntimeHeader, RuntimeRequestWire, RuntimeResponseWire,
    RuntimeStateStoreConfig, RuntimeTransportConfig,
};

use crate::types::{
    FlutterChunkWire, FlutterHeader, FlutterPersistedState, FlutterRequest,
    FlutterRequestJournalEntry, FlutterResponse, FlutterRuntimeCodec, FlutterRuntimeConfig,
    FlutterRuntimeEnvelope, FlutterRuntimeError, FlutterStateStoreConfig,
};

impl From<RuntimeChunkWire> for FlutterChunkWire {
    fn from(value: RuntimeChunkWire) -> Self {
        match value {
            RuntimeChunkWire::Item(bytes) => Self::Item(bytes),
            RuntimeChunkWire::End => Self::End,
            RuntimeChunkWire::Error(error) => Self::Error(FlutterRuntimeError::from(error)),
        }
    }
}

impl From<FlutterRuntimeConfig> for RuntimeConfigWire {
    fn from(value: FlutterRuntimeConfig) -> Self {
        Self {
            base_url: value.base_url,
            state_store: match value.state_store {
                FlutterStateStoreConfig::InMemory => RuntimeStateStoreConfig::InMemory,
                FlutterStateStoreConfig::JsonFile { path } => RuntimeStateStoreConfig::JsonFile {
                    path: PathBuf::from(path),
                },
            },
            transport: RuntimeTransportConfig {
                codec: match value.transport.codec {
                    FlutterRuntimeCodec::Cbor => RuntimeCodecConfig::Cbor,
                    FlutterRuntimeCodec::Json => RuntimeCodecConfig::Json,
                },
                envelope: match value.transport.envelope {
                    FlutterRuntimeEnvelope::None => RuntimeEnvelopeConfig::None,
                    FlutterRuntimeEnvelope::CoseSign1 => RuntimeEnvelopeConfig::CoseSign1,
                },
            },
        }
    }
}

impl From<FlutterHeader> for RuntimeHeader {
    fn from(value: FlutterHeader) -> Self {
        Self {
            name: value.name,
            value: value.value,
        }
    }
}

impl From<RuntimeHeader> for FlutterHeader {
    fn from(value: RuntimeHeader) -> Self {
        Self {
            name: value.name,
            value: value.value,
        }
    }
}

impl From<FlutterRequest> for RuntimeRequestWire {
    fn from(value: FlutterRequest) -> Self {
        Self {
            method: value.method,
            path: value.path,
            canonical_query: value.canonical_query,
            headers: value.headers.into_iter().map(RuntimeHeader::from).collect(),
            body: value.body,
        }
    }
}

impl From<RuntimeResponseWire> for FlutterResponse {
    fn from(value: RuntimeResponseWire) -> Self {
        Self {
            status_code: value.status_code,
            headers: value.headers.into_iter().map(FlutterHeader::from).collect(),
            body: value.body,
        }
    }
}

impl From<cratestack_client_rust::PersistedClientState> for FlutterPersistedState {
    fn from(value: cratestack_client_rust::PersistedClientState) -> Self {
        Self {
            schema_version: value.schema_version,
            state_version: value.state_version,
            request_journal: value
                .request_journal
                .into_iter()
                .map(|entry| FlutterRequestJournalEntry {
                    method: entry.method,
                    path: entry.path,
                    status_code: entry.status_code,
                    content_type: entry.content_type,
                    recorded_at_rfc3339: entry.recorded_at.to_rfc3339(),
                })
                .collect(),
        }
    }
}

impl From<RuntimeErrorWire> for FlutterRuntimeError {
    fn from(value: RuntimeErrorWire) -> Self {
        Self {
            code: value.code as u32,
            http_status: value.http_status,
            message: value.message,
            remote_code: value.remote_code,
            remote_body: value.remote_body,
        }
    }
}

impl From<ClientError> for FlutterRuntimeError {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Transport(error) => Self {
                code: RuntimeErrorCode::Transport as u32,
                http_status: None,
                message: error.to_string(),
                remote_code: None,
                remote_body: None,
            },
            ClientError::Codec(error) => Self {
                code: RuntimeErrorCode::Codec as u32,
                http_status: Some(error.status_code().as_u16()),
                message: error.to_string(),
                remote_code: Some(error.code().to_owned()),
                remote_body: None,
            },
            ClientError::State(message) => Self {
                code: RuntimeErrorCode::State as u32,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::InvalidResponse(message) => Self {
                code: RuntimeErrorCode::InvalidResponse as u32,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::BadInput(message) => Self {
                code: RuntimeErrorCode::BadInput as u32,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::Remote {
                status,
                error,
                message,
            } => Self {
                code: RuntimeErrorCode::Remote as u32,
                http_status: Some(status.as_u16()),
                remote_code: error.as_ref().map(|value| value.code.clone()),
                remote_body: error
                    .as_ref()
                    .and_then(|value| serde_json::to_vec(value).ok()),
                message,
            },
        }
    }
}

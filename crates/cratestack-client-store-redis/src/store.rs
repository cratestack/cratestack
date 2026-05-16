//! Redis-backed implementation of `ClientStateStore`.

use chrono::Utc;
use cratestack_client_rust::{
    ClientError, ClientStateStore, PersistedClientState, RequestJournalEntry,
};
use redis::Commands;

use crate::config::RedisStateStoreConfig;

const REDIS_SCHEMA_VERSION: u32 = 1;

pub struct RedisStateStore {
    client: redis::Client,
    config: RedisStateStoreConfig,
}

impl RedisStateStore {
    pub fn open(
        redis_url: impl redis::IntoConnectionInfo,
        key_prefix: impl Into<String>,
    ) -> Result<Self, ClientError> {
        let client = redis::Client::open(redis_url).map_err(redis_error)?;
        Ok(Self::from_client(client, key_prefix))
    }

    pub fn from_client(client: redis::Client, key_prefix: impl Into<String>) -> Self {
        Self {
            client,
            config: RedisStateStoreConfig::new(key_prefix),
        }
    }

    pub fn key_prefix(&self) -> &str {
        &self.config.key_prefix
    }

    pub fn meta_key(&self) -> String {
        self.config.meta_key()
    }

    pub fn request_journal_key(&self) -> String {
        self.config.request_journal_key()
    }

    fn connection(&self) -> Result<redis::Connection, ClientError> {
        self.client.get_connection().map_err(redis_error)
    }

    fn bootstrap(&self, connection: &mut redis::Connection) -> Result<(), ClientError> {
        let meta_key = self.config.meta_key();
        let exists: bool = connection.exists(&meta_key).map_err(redis_error)?;
        if !exists {
            let _: () = redis::pipe()
                .atomic()
                .hset(&meta_key, "schema_version", REDIS_SCHEMA_VERSION)
                .ignore()
                .hset(&meta_key, "state_version", 0_u64)
                .ignore()
                .hset(&meta_key, "updated_at", Utc::now().to_rfc3339())
                .ignore()
                .query(connection)
                .map_err(redis_error)?;
        }
        Ok(())
    }
}

impl ClientStateStore for RedisStateStore {
    fn load(&self) -> Result<PersistedClientState, ClientError> {
        let mut connection = self.connection()?;
        self.bootstrap(&mut connection)?;

        let meta_key = self.config.meta_key();
        let journal_key = self.config.request_journal_key();
        let (schema_version, state_version): (u32, u64) = redis::pipe()
            .hget(&meta_key, "schema_version")
            .hget(&meta_key, "state_version")
            .query(&mut connection)
            .map_err(redis_error)?;
        let entries: Vec<String> = connection
            .lrange(&journal_key, 0, -1)
            .map_err(redis_error)?;
        let request_journal = entries
            .into_iter()
            .map(|entry| {
                serde_json::from_str::<RequestJournalEntry>(&entry).map_err(|error| {
                    ClientError::State(format!(
                        "failed to decode Redis request journal entry from {journal_key}: {error}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PersistedClientState {
            schema_version,
            state_version,
            request_journal,
        })
    }

    fn save(&self, state: &PersistedClientState) -> Result<(), ClientError> {
        let mut connection = self.connection()?;
        let meta_key = self.config.meta_key();
        let journal_key = self.config.request_journal_key();
        let mut pipe = redis::pipe();
        pipe.atomic()
            .del(&journal_key)
            .ignore()
            .hset(&meta_key, "schema_version", state.schema_version)
            .ignore()
            .hset(&meta_key, "state_version", state.state_version)
            .ignore()
            .hset(&meta_key, "updated_at", Utc::now().to_rfc3339());
        pipe.ignore();

        for entry in &state.request_journal {
            pipe.rpush(&journal_key, encode_entry(entry)?);
            pipe.ignore();
        }

        let _: () = pipe.query(&mut connection).map_err(redis_error)?;
        Ok(())
    }

    fn append_request_journal(&self, entry: &RequestJournalEntry) -> Result<(), ClientError> {
        let mut connection = self.connection()?;
        self.bootstrap(&mut connection)?;
        let meta_key = self.config.meta_key();
        let journal_key = self.config.request_journal_key();
        let _: () = redis::pipe()
            .atomic()
            .rpush(&journal_key, encode_entry(entry)?)
            .ignore()
            .hincr(&meta_key, "state_version", 1_u64)
            .ignore()
            .hset(&meta_key, "updated_at", Utc::now().to_rfc3339())
            .ignore()
            .query(&mut connection)
            .map_err(redis_error)?;
        Ok(())
    }
}

pub(crate) fn encode_entry(entry: &RequestJournalEntry) -> Result<String, ClientError> {
    serde_json::to_string(entry).map_err(|error| {
        ClientError::State(format!("failed to encode Redis journal entry: {error}"))
    })
}

fn redis_error(error: redis::RedisError) -> ClientError {
    ClientError::State(format!("Redis state store error: {error}"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use cratestack_client_rust::RequestJournalEntry;

    use super::{RedisStateStore, encode_entry};

    #[test]
    fn empty_key_prefix_uses_default_namespace() {
        let client = redis::Client::open("redis://127.0.0.1/")
            .expect("static Redis URL should parse without connecting");
        let store = RedisStateStore::from_client(client, "::");

        assert_eq!(store.key_prefix(), "cratestack:client");
        assert_eq!(store.meta_key(), "cratestack:client:meta");
        assert_eq!(
            store.request_journal_key(),
            "cratestack:client:request_journal"
        );
    }

    #[test]
    fn journal_entry_encodes_as_json() {
        let encoded = encode_entry(&RequestJournalEntry {
            method: "POST".to_owned(),
            path: "/$procs/getFeed".to_owned(),
            status_code: 200,
            content_type: Some("application/cbor".to_owned()),
            recorded_at: Utc::now(),
        })
        .expect("entry should encode");

        assert!(encoded.contains(r#""method":"POST""#));
        assert!(encoded.contains(r#""status_code":200"#));
    }
}

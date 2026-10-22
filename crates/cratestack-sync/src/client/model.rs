//! How the engine writes server rows into a local table, and the local
//! mutators that apply a mutation optimistically.

use std::marker::PhantomData;
use std::str::FromStr;

use cratestack_rusqlite::rusqlite::{Connection, params_from_iter};
use cratestack_rusqlite::{
    ConflictTarget, IntoSqlValue, ModelDescriptor, SqlValueParam, SqliteDialect, UpsertModelInput,
    render_upsert_with_conflict,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::ClientError;

/// A synced model as the client engine needs it.
pub trait SyncedModel: Send + Sync {
    /// Name used in [`crate::protocol::Change::model`].
    fn model(&self) -> &str;
    fn table(&self) -> &str;
    fn pk_column(&self) -> &str;
    /// Write a server row (insert or overwrite).
    fn upsert(&self, conn: &Connection, row: &Value) -> Result<(), ClientError>;
    /// Hard-delete by primary key. A server delete is final locally, even
    /// for an `@@soft_delete` model: the server decided the client may no
    /// longer see the row.
    fn delete(&self, conn: &Connection, pk: &str) -> Result<(), ClientError>;
}

/// [`SyncedModel`] for an `include_embedded_schema!` model, driven by its
/// generated descriptor. `I` is the generated `Create<Model>Input`: the
/// server row deserializes into it, and its `UpsertModelInput` impl renders
/// the same canonical storage encoding every other embedded write uses.
pub struct DescriptorModel<M: 'static, PK: 'static, I> {
    model: &'static str,
    descriptor: &'static ModelDescriptor<M, PK>,
    _input: PhantomData<fn() -> I>,
}

impl<M: 'static, PK: 'static, I> DescriptorModel<M, PK, I> {
    pub fn new(model: &'static str, descriptor: &'static ModelDescriptor<M, PK>) -> Self {
        Self {
            model,
            descriptor,
            _input: PhantomData,
        }
    }
}

impl<M, PK, I> SyncedModel for DescriptorModel<M, PK, I>
where
    M: Send + Sync + 'static,
    PK: FromStr + IntoSqlValue + Send + Sync + 'static,
    I: UpsertModelInput<M> + DeserializeOwned,
{
    fn model(&self) -> &str {
        self.model
    }

    fn table(&self) -> &str {
        self.descriptor.table_name
    }

    fn pk_column(&self) -> &str {
        self.descriptor.primary_key
    }

    fn upsert(&self, conn: &Connection, row: &Value) -> Result<(), ClientError> {
        // Deserialize from the borrowed value: no copy of the row.
        let input = I::deserialize(row)?;
        let values = <I as UpsertModelInput<M>>::sql_values(&input);
        let (sql, binds) = render_upsert_with_conflict(
            &SqliteDialect,
            self.descriptor,
            &values,
            ConflictTarget::PRIMARY_KEY,
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(binds.iter().map(SqlValueParam)))?;
        while rows.next()?.is_some() {}
        Ok(())
    }

    fn delete(&self, conn: &Connection, pk: &str) -> Result<(), ClientError> {
        let pk = PK::from_str(pk)
            .map_err(|_| ClientError::Protocol(format!("bad {} primary key `{pk}`", self.model)))?
            .into_sql_value();
        let sql = format!(
            "DELETE FROM {} WHERE {} = ?1",
            self.descriptor.table_name, self.descriptor.primary_key
        );
        conn.execute(&sql, [SqlValueParam(&pk)])?;
        Ok(())
    }
}

/// Applies a mutation to the local database, optimistically. Must make the
/// same change the server handler of the same name makes — it is replayed
/// on every rebase, so it must also be a pure function of `(db, args)`.
pub trait LocalMutator: Send + Sync {
    fn apply(&self, conn: &Connection, args: &Value) -> Result<(), ClientError>;
}

impl<F> LocalMutator for F
where
    F: Fn(&Connection, &Value) -> Result<(), ClientError> + Send + Sync,
{
    fn apply(&self, conn: &Connection, args: &Value) -> Result<(), ClientError> {
        self(conn, args)
    }
}

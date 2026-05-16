use super::sql_value::{SqlColumnValue, SqlValue};

pub trait CreateModelInput<M> {
    fn sql_values(&self) -> Vec<SqlColumnValue>;
    /// Run schema-derived validators (`@length`, `@email`, `@regex`, ...) on
    /// the input. Default impl is a no-op for inputs without validators.
    fn validate(&self) -> Result<(), cratestack_core::CoolError> {
        Ok(())
    }
}

pub trait UpdateModelInput<M> {
    fn sql_values(&self) -> Vec<SqlColumnValue>;
    fn validate(&self) -> Result<(), cratestack_core::CoolError> {
        Ok(())
    }
}

/// Input shape for the upsert primitive — `INSERT … ON CONFLICT (<pk>) DO
/// UPDATE …`. `sql_values()` must include the primary-key column (so the
/// backend can target the conflict), and `primary_key_value()` exposes the
/// PK separately so the runtime can issue a `SELECT … FOR UPDATE` before
/// the upsert to drive `Created` vs. `Updated` event / audit semantics.
///
/// Only models with a client-supplied primary key (i.e. `@id` *without*
/// `@default(...)`) emit this trait impl; models with server-generated PKs
/// don't get an `.upsert()` builder at all. That's intentional — at v1 the
/// upsert primitive is PK-conflict only, and a server-generated PK can't be
/// upserted without the caller supplying one anyway.
pub trait UpsertModelInput<M>: Send {
    /// Full set of column→value bindings, *including* the primary key.
    fn sql_values(&self) -> Vec<SqlColumnValue>;

    /// The primary-key value, used to issue the `SELECT … FOR UPDATE` probe
    /// inside the upsert transaction. Must match the PK column carried in
    /// `sql_values()`.
    fn primary_key_value(&self) -> SqlValue;

    fn validate(&self) -> Result<(), cratestack_core::CoolError> {
        Ok(())
    }
}

/// Accessor for a model's primary key. Implemented by the macro on every
/// generated model struct so the batch operations can pair returned rows
/// back to the position of their input PK in the request, producing a
/// `BatchItemResult` with the right `index` and a `NotFound` entry for any
/// requested PK that didn't come back.
pub trait ModelPrimaryKey<PK> {
    fn primary_key(&self) -> PK;
}

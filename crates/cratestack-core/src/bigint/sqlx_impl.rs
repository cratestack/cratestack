//! Postgres driver impls for [`BigInt`], behind the `sqlx-postgres` feature.
//!
//! Each delegates to `i64`, so a `BigInt` is `INT8` on the wire exactly as it
//! is a `BIGINT` column. They live here, and not in `cratestack-sqlx`,
//! because of the orphan rule: only the crate that owns `BigInt` or the crate
//! that owns the sqlx traits may implement them, and a `BigInt @id` or a
//! `BigInt` foreign key reaches the generic `PK: sqlx::Type<Postgres> +
//! Encode<Postgres>` bounds of the delegates directly.
//!
//! This is what `Vec<BigInt>`, `&[BigInt]` and `Option<BigInt>` need too:
//! sqlx builds those from `Type`, `Encode`, `Decode` and `PgHasArrayType`.

use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;
use sqlx_postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueRef, Postgres};

use super::BigInt;

impl Type<Postgres> for BigInt {
    fn type_info() -> PgTypeInfo {
        <i64 as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <i64 as Type<Postgres>>::compatible(ty)
    }
}

impl PgHasArrayType for BigInt {
    fn array_type_info() -> PgTypeInfo {
        <i64 as PgHasArrayType>::array_type_info()
    }

    fn array_compatible(ty: &PgTypeInfo) -> bool {
        <i64 as PgHasArrayType>::array_compatible(ty)
    }
}

impl Encode<'_, Postgres> for BigInt {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        <i64 as Encode<'_, Postgres>>::encode_by_ref(&self.0, buf)
    }

    fn size_hint(&self) -> usize {
        <i64 as Encode<'_, Postgres>>::size_hint(&self.0)
    }
}

impl Decode<'_, Postgres> for BigInt {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        <i64 as Decode<'_, Postgres>>::decode(value).map(Self)
    }
}

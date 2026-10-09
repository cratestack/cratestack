//! The Postgres impls are `INT8` by delegation. The compile-time half is the
//! point: a `BigInt @id` reaches `PK: Type<Postgres> + Encode<Postgres>`
//! bounds, and `Option`, `Vec` and slices of it reach the array impls.

use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::type_info::TypeInfo as _;
use sqlx_core::types::Type;
use sqlx_postgres::{PgArgumentBuffer, PgHasArrayType, Postgres};

use super::BigInt;
use super::tests::BOUNDARIES;

fn assert_pg_scalar<T>()
where
    T: Type<Postgres> + for<'q> Encode<'q, Postgres> + for<'r> Decode<'r, Postgres>,
{
}

fn assert_pg_bindable<T>()
where
    T: Type<Postgres> + for<'q> Encode<'q, Postgres>,
{
}

#[test]
fn a_big_int_is_a_postgres_scalar_option_vec_and_slice() {
    assert_pg_scalar::<BigInt>();
    assert_pg_scalar::<Option<BigInt>>();
    assert_pg_scalar::<Vec<BigInt>>();
    assert_pg_bindable::<&BigInt>();
    assert_pg_bindable::<&[BigInt]>();
}

#[test]
fn the_postgres_type_is_int8_and_its_array() {
    assert_eq!(<BigInt as Type<Postgres>>::type_info().name(), "INT8");
    assert_eq!(
        <BigInt as PgHasArrayType>::array_type_info().name(),
        "INT8[]"
    );
    assert_eq!(
        <BigInt as Type<Postgres>>::type_info(),
        <i64 as Type<Postgres>>::type_info()
    );
    assert!(<BigInt as Type<Postgres>>::compatible(&<i64 as Type<
        Postgres,
    >>::type_info()));
    assert!(!<BigInt as Type<Postgres>>::compatible(&<i32 as Type<
        Postgres,
    >>::type_info(
    )));
}

#[test]
fn encoding_is_eight_big_endian_bytes_at_the_boundaries() {
    for (value, _) in BOUNDARIES {
        let mut buffer = PgArgumentBuffer::default();
        let null = Encode::<Postgres>::encode_by_ref(&BigInt::new(value), &mut buffer).unwrap();
        assert!(matches!(null, IsNull::No));
        assert_eq!(buffer.as_slice(), value.to_be_bytes());
        assert_eq!(Encode::<Postgres>::size_hint(&BigInt::new(value)), 8);
    }
}

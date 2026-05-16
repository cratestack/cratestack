#![cfg(test)]

use cratestack_sql::{FilterValue, OrderTarget};

use crate::{FieldRef, SortDirection, SqlValue};

#[test]
fn field_ref_builds_filter_and_order() {
    let filter = FieldRef::<(), bool>::new("published").is_true();
    let order = FieldRef::<(), String>::new("title").desc();
    let contains = FieldRef::<(), String>::new("title").contains("hel");
    let maybe_null = FieldRef::<(), Option<String>>::new("subtitle").is_null();
    let in_filter = FieldRef::<(), i64>::new("id").in_([1_i64, 2_i64]);

    assert_eq!(filter.column, "published");
    assert_eq!(filter.value, FilterValue::Single(SqlValue::Bool(true)));
    assert_eq!(order.direction, SortDirection::Desc);
    assert!(matches!(order.target, OrderTarget::Column("title")));
    assert_eq!(
        contains.value,
        FilterValue::Single(SqlValue::String("%hel%".to_owned()))
    );
    assert_eq!(maybe_null.column, "subtitle");
    assert_eq!(
        in_filter.value,
        FilterValue::Many(vec![SqlValue::Int(1), SqlValue::Int(2)])
    );
}

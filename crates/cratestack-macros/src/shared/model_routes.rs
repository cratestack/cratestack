//! A model's REST route templates, derived once.
//!
//! The server lists them in `ROUTE_TRANSPORTS` and a signed client binds them
//! into its COSE envelope (ADR 0006 §4, cratestack#1007), and the two must be
//! the same string: a request sealed for `/widgets/{id}` is refused by a
//! server that registered `/widgets/{widget_id}`. So both call these.

/// `/widgets`: the collection route.
pub(crate) fn model_list_path(model_name: &str) -> String {
    cratestack_core::model_list_route(model_name)
}

/// `/widgets/{id}`: the detail route.
pub(crate) fn model_detail_path(model_name: &str) -> String {
    cratestack_core::model_detail_route(model_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_is_the_list_route_and_an_id() {
        assert_eq!(model_list_path("Widget"), "/widgets");
        assert_eq!(model_detail_path("Widget"), "/widgets/{id}");
        assert_eq!(model_detail_path("BankAccount"), "/bank_accounts/{id}");
    }
}

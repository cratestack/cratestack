//! cratestack#1123: `cratestack_core::client_contract::ops` is the op list a
//! signed client keys its contract by, so it must be exactly what the macro
//! emits: `OPS` on `transport rpc`, `ROUTE_TRANSPORTS` (`"<METHOD> <path>"`)
//! on REST. A divergence would bind a digest to an op the server never
//! routes, or leave a routed op with no digest.

use std::collections::BTreeSet;

use cratestack::include_server_schema;

fn op_keys(path: &str) -> BTreeSet<String> {
    let schema = cratestack_parser::parse_schema_file(path).expect("fixture parses");
    cratestack_core::client_contract::ops(&schema)
        .into_iter()
        .map(|op| op.key)
        .collect()
}

fn rpc_keys(ops: &[cratestack::OpDescriptor]) -> BTreeSet<String> {
    ops.iter().map(|op| op.op_id.to_owned()).collect()
}

fn rest_keys(routes: &[cratestack::RouteTransportDescriptor]) -> BTreeSet<String> {
    routes
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect()
}

mod rest {
    use super::*;
    include_server_schema!("tests/fixtures/op_contract_rest.cstack", db = Postgres);

    #[test]
    fn ops_equal_route_transports() {
        let emitted = rest_keys(cratestack_schema::axum::ROUTE_TRANSPORTS);
        assert!(emitted.contains("POST /v2/$procs/ping"), "{emitted:?}");
        assert!(emitted.contains("GET /bank__accounts"), "{emitted:?}");
        assert!(!emitted.contains("POST /widgets"));
        assert!(!emitted.iter().any(|k| k.contains("gadgets")));
        assert_eq!(op_keys("tests/fixtures/op_contract_rest.cstack"), emitted);
    }
}

mod rpc {
    use super::*;
    include_server_schema!("tests/fixtures/transport_rpc.cstack", db = Postgres);

    #[test]
    fn ops_equal_op_descriptors_including_subscribe_and_sequences() {
        let emitted = rpc_keys(cratestack_schema::axum::OPS);
        assert!(emitted.contains("model.Widget.subscribe"));
        assert_eq!(op_keys("tests/fixtures/transport_rpc.cstack"), emitted);
    }
}

mod rpc_suppressed {
    use super::*;
    include_server_schema!(
        "tests/fixtures/internal_suppression_rpc.cstack",
        db = Postgres
    );

    #[test]
    fn internal_verbs_are_absent_from_both_lists() {
        let emitted = rpc_keys(cratestack_schema::axum::OPS);
        assert_eq!(
            op_keys("tests/fixtures/internal_suppression_rpc.cstack"),
            emitted
        );
    }
}

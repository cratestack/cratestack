//! `SUBSCRIPTION_FRAMES` (cratestack#1378, part A): the generated table lists
//! exactly the subscriptions a schema marks `@frames(unsigned)`, keyed by the
//! op key the contract tables use, on a REST and an RPC schema alike. An
//! unmarked subscription and a unary procedure have no row, and a schema
//! with no mark emits an empty table.

use cratestack::FramesMode;

mod rpc {
    cratestack::include_server_schema!("tests/fixtures/subscription_frames_rpc.cstack", db = None);
}

mod rest {
    cratestack::include_server_schema!("tests/fixtures/subscription_frames_rest.cstack", db = None);
}

mod unmarked {
    cratestack::include_server_schema!(
        "tests/fixtures/subscription_procedure_no_database.cstack",
        db = None
    );
}

#[test]
fn an_rpc_schema_lists_exactly_the_marked_subscription() {
    assert_eq!(
        rpc::cratestack_schema::SUBSCRIPTION_FRAMES,
        &[("procedure.hinted", FramesMode::Unsigned)]
    );
}

#[test]
fn a_rest_schema_lists_exactly_the_marked_subscription() {
    assert_eq!(
        rest::cratestack_schema::SUBSCRIPTION_FRAMES,
        &[("GET /$procs/hinted", FramesMode::Unsigned)]
    );
}

#[test]
fn every_listed_key_is_an_op_the_schema_has_and_a_contract_row_exists() {
    for (table, contracts) in [
        (
            rpc::cratestack_schema::SUBSCRIPTION_FRAMES,
            rpc::cratestack_schema::ACCEPTED_CONTRACTS,
        ),
        (
            rest::cratestack_schema::SUBSCRIPTION_FRAMES,
            rest::cratestack_schema::ACCEPTED_CONTRACTS,
        ),
    ] {
        for (key, _) in table {
            assert!(contracts.iter().any(|(k, _)| k == key), "{key}");
        }
    }
}

#[test]
fn a_schema_with_no_mark_emits_an_empty_table() {
    assert!(unmarked::cratestack_schema::SUBSCRIPTION_FRAMES.is_empty());
}

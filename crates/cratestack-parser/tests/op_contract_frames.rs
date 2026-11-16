//! `@frames(unsigned)` is in the op contract (cratestack#1378, part A): it is
//! not on the drop list, so adding or removing it moves that subscription's
//! digest and nobody else's, and the compat classifier fails closed in both
//! directions (maintainer decision 4).

mod common;
use common::*;

use cratestack_core::{Verdict, classify, op_contract_json};

const KEY: &str = "procedure.feed";

fn source(attributes: &str) -> String {
    format!(
        "{BASE}\ntype Tick {{\n  n Int\n}}\n\nsubscription procedure feed(args: PingArgs): Tick\n  \
         @allow(auth() != null){attributes}\n"
    )
}

fn contract(source: &str) -> serde_json::Value {
    let schema = cratestack_parser::parse_schema(source).expect("parses");
    serde_json::from_str(&op_contract_json(&schema, KEY).expect("op")).expect("json")
}

#[test]
fn adding_frames_moves_that_subscriptions_digest_only() {
    let (plain, marked) = (
        digests(&source("")),
        digests(&source("\n  @frames(unsigned)")),
    );
    assert_ne!(plain[KEY], marked[KEY], "the digest must move");
    for (key, digest) in &plain {
        if key != KEY {
            assert_eq!(marked.get(key), Some(digest), "{key} must not move");
        }
    }
}

#[test]
fn removing_frames_restores_the_digest() {
    let plain = digests(&source(""));
    let marked = digests(&source("\n  @frames(unsigned)"));
    assert_ne!(marked[KEY], plain[KEY]);
    assert_eq!(digests(&source(""))[KEY], plain[KEY]);
}

#[test]
fn adding_frames_is_wire_incompatible() {
    let verdict = classify(
        &contract(&source("")),
        &contract(&source("\n  @frames(unsigned)")),
    );
    let Verdict::Breaking(reasons) = verdict else {
        panic!("adding @frames must fail closed");
    };
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("gained the attribute @frames(unsigned)")
                && r.contains("trust decision")),
        "{reasons:?}"
    );
}

#[test]
fn removing_frames_is_wire_incompatible() {
    let verdict = classify(
        &contract(&source("\n  @frames(unsigned)")),
        &contract(&source("")),
    );
    let Verdict::Breaking(reasons) = verdict else {
        panic!("removing @frames must fail closed");
    };
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("lost the attribute @frames(unsigned)")
                && r.contains("trust decision")),
        "{reasons:?}"
    );
}

#[test]
fn the_same_frames_is_compatible() {
    let marked = contract(&source("\n  @frames(unsigned)"));
    assert!(classify(&marked, &marked).is_compatible());
}

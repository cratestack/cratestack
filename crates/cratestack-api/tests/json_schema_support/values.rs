//! Representative values of the generated types. Each list spans the edges
//! of its scalar's serde output: extremes, empty, unicode, fractional
//! seconds, pre-epoch times, and every decimal form the backend emits.

use cratestack::chrono::{DateTime, Utc};
use cratestack::uuid::Uuid;

use crate::cratestack_schema::{Scalars, Shapes, Status, Tree};

fn at(seconds: i64, nanos: u32) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, nanos).expect("in chrono's range")
}

/// Every value of every scalar appears at least once; fields cycle
/// independently so the combinations vary too.
pub fn scalars() -> Vec<Scalars> {
    let texts = ["", "plain", "héllo ✓ \"quoted\"\n\ttabbed"];
    let cuids = ["ckabc123", ""];
    let counts = [i64::MIN, -1, 0, 1, i64::MAX];
    let ratios = [
        0.0,
        -1.5,
        3.0,
        1e308,
        f64::MIN_POSITIVE,
        -2.5e-300,
        123_456_789.0,
    ];
    let ats = [
        at(0, 0),
        at(1_700_000_000, 123_456_789),
        at(-86_400, 1),
        at(253_402_300_799, 999_999_999),
    ];
    let blobs: [Vec<u8>; 3] = [vec![], vec![0, 255], (0..=255).collect()];
    let ids = [
        Uuid::nil(),
        Uuid::max(),
        Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
    ];
    let statuses = [Status::Active, Status::Archived];
    let decimals = crate::DECIMAL.samples;
    let count = [
        texts.len(),
        counts.len(),
        ratios.len(),
        ats.len(),
        decimals.len(),
    ]
    .into_iter()
    .max()
    .unwrap();
    (0..count)
        .map(|i| Scalars {
            text: texts[i % texts.len()].to_owned(),
            cuid: cuids[i % cuids.len()].to_owned(),
            count: counts[i % counts.len()],
            ratio: ratios[i % ratios.len()],
            flag: i % 2 == 0,
            at: ats[i % ats.len()],
            amount: decimals[i % decimals.len()]
                .parse()
                .unwrap_or_else(|error| {
                    panic!(
                        "decimal sample {:?}: {error:?}",
                        decimals[i % decimals.len()]
                    )
                }),
            blob: blobs[i % blobs.len()].clone(),
            id: ids[i % ids.len()],
            status: statuses[i % statuses.len()],
        })
        .collect()
}

/// All optionals `None` and all lists empty, then everything populated.
pub fn shapes() -> Vec<Shapes> {
    let samples = scalars();
    let empty = Shapes {
        label: None,
        tags: Vec::new(),
        statuses: Vec::new(),
        maybeStatus: None,
        child: samples[0].clone(),
        children: Vec::new(),
        maybeChild: None,
        blobs: Vec::new(),
        maybeBlob: None,
        maybeAmount: None,
        maybeAt: None,
        ids: Vec::new(),
    };
    let full = Shapes {
        label: Some("label".to_owned()),
        tags: vec!["a".to_owned(), String::new()],
        statuses: vec![Status::Archived, Status::Active],
        maybeStatus: Some(Status::Archived),
        child: samples[1].clone(),
        children: samples.clone(),
        maybeChild: Some(samples[2].clone()),
        blobs: vec![vec![], vec![7, 8, 9]],
        maybeBlob: Some(vec![1, 2, 3]),
        // Parsed rather than cloned from `samples`: `RustDecimal` is
        // `Copy` and `BigDecimal` isn't, so neither spelling lints clean
        // under both backends.
        maybeAmount: Some(crate::DECIMAL.samples[3].parse().unwrap()),
        maybeAt: Some(samples[3].at),
        ids: samples.iter().map(|s| s.id).collect(),
    };
    vec![empty, full]
}

pub fn tree() -> Tree {
    let leaf = |label: &str| Tree {
        label: label.to_owned(),
        children: Vec::new(),
    };
    Tree {
        label: "root".to_owned(),
        children: vec![
            leaf("a"),
            Tree {
                label: "b".to_owned(),
                children: vec![leaf("b1"), leaf("b2")],
            },
        ],
    }
}
